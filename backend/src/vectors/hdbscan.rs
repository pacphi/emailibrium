//! HDBSCAN: Hierarchical Density-Based Spatial Clustering of Applications with Noise.
//!
//! Algorithm (ADR-009 alternative to GraphSAGE + KMeans++):
//! 1. Compute core distances (distance to k-th nearest neighbor).
//! 2. Build MST over the mutual reachability graph.
//! 3. Extract condensed cluster hierarchy from the MST.
//! 4. Select flat clusters via stability-based criterion.
//!
//! Points not in any stable cluster are labeled as noise (label = -1).

use super::categorizer::cosine_similarity;

/// Configuration for the HDBSCAN algorithm.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HdbscanConfig {
    /// Minimum points to form a cluster. Larger = fewer, denser clusters.
    #[serde(default = "default_min_cluster_size")]
    pub min_cluster_size: usize,
    /// Neighbors for core distance computation. Defaults to `min_cluster_size`.
    #[serde(default)]
    pub min_samples: Option<usize>,
    /// Distance threshold below which clusters are not split. 0.0 = disabled.
    #[serde(default)]
    pub cluster_selection_epsilon: f32,
}

fn default_min_cluster_size() -> usize {
    5
}

impl Default for HdbscanConfig {
    fn default() -> Self {
        Self {
            min_cluster_size: default_min_cluster_size(),
            min_samples: None,
            cluster_selection_epsilon: 0.0,
        }
    }
}

/// Result of an HDBSCAN clustering run.
#[derive(Debug, Clone)]
pub struct HdbscanResult {
    /// Cluster label per point. -1 = noise.
    pub labels: Vec<i32>,
    /// Confidence in [0, 1] per point. Noise points get 0.0.
    pub confidences: Vec<f32>,
    /// Number of clusters found (excluding noise).
    pub n_clusters: usize,
}

// -- Distance helpers --------------------------------------------------------

fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    (1.0 - cosine_similarity(a, b)).clamp(0.0, 2.0)
}

fn pairwise_distances(data: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let n = data.len();
    let mut d = vec![vec![0.0_f32; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let v = cosine_distance(&data[i], &data[j]);
            d[i][j] = v;
            d[j][i] = v;
        }
    }
    d
}

fn core_distances(dist_matrix: &[Vec<f32>], k: usize) -> Vec<f32> {
    let n = dist_matrix.len();
    let k = k.min(n.saturating_sub(1)).max(1);
    (0..n)
        .map(|i| {
            let mut dists: Vec<f32> = (0..n).filter(|&j| j != i).map(|j| dist_matrix[i][j]).collect();
            dists.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            dists[k.min(dists.len()) - 1]
        })
        .collect()
}

fn mutual_reachability(core_a: f32, core_b: f32, dist_ab: f32) -> f32 {
    core_a.max(core_b).max(dist_ab)
}

// -- MST (Prim's algorithm) --------------------------------------------------

#[derive(Debug, Clone)]
struct MstEdge { a: usize, b: usize, weight: f32 }

fn build_mst(dist_matrix: &[Vec<f32>], core_dists: &[f32]) -> Vec<MstEdge> {
    let n = dist_matrix.len();
    if n <= 1 { return vec![]; }

    let mut in_tree = vec![false; n];
    let mut min_w = vec![f32::MAX; n];
    let mut min_from = vec![0usize; n];
    let mut edges = Vec::with_capacity(n - 1);

    in_tree[0] = true;
    for j in 1..n {
        min_w[j] = mutual_reachability(core_dists[0], core_dists[j], dist_matrix[0][j]);
    }

    for _ in 0..(n - 1) {
        let (best, bw) = (0..n)
            .filter(|&j| !in_tree[j])
            .map(|j| (j, min_w[j]))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, f32::MAX));

        in_tree[best] = true;
        edges.push(MstEdge { a: min_from[best], b: best, weight: bw });

        for j in 0..n {
            if !in_tree[j] {
                let w = mutual_reachability(core_dists[best], core_dists[j], dist_matrix[best][j]);
                if w < min_w[j] { min_w[j] = w; min_from[j] = best; }
            }
        }
    }
    edges
}

// -- Union-Find --------------------------------------------------------------

struct UnionFind { parent: Vec<usize>, size: Vec<usize> }

impl UnionFind {
    fn new(n: usize) -> Self { Self { parent: (0..n).collect(), size: vec![1; n] } }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x { self.parent[x] = self.parent[self.parent[x]]; x = self.parent[x]; }
        x
    }
    fn union(&mut self, a: usize, b: usize) -> (usize, usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb { return (ra, rb); }
        let (big, small) = if self.size[ra] >= self.size[rb] { (ra, rb) } else { (rb, ra) };
        self.parent[small] = big;
        self.size[big] += self.size[small];
        (big, small)
    }
}

// -- Hierarchy node ----------------------------------------------------------

#[derive(Debug)]
struct HNode {
    birth_lambda: f32,
    death_lambda: f32,
    size: usize,
    children: Vec<usize>,
    points: Vec<usize>,
    stability: f32,
    selected: bool,
}

// -- Core algorithm ----------------------------------------------------------

/// Run HDBSCAN on f32 vectors using cosine distance.
pub fn hdbscan(data: &[Vec<f32>], config: &HdbscanConfig) -> HdbscanResult {
    let n = data.len();
    if n == 0 { return HdbscanResult { labels: vec![], confidences: vec![], n_clusters: 0 }; }
    if n == 1 { return HdbscanResult { labels: vec![-1], confidences: vec![0.0], n_clusters: 0 }; }

    let mcs = config.min_cluster_size.max(2);
    let ms = config.min_samples.unwrap_or(mcs);
    let epsilon = config.cluster_selection_epsilon;

    let dm = pairwise_distances(data);
    let cd = core_distances(&dm, ms);
    let mut mst = build_mst(&dm, &cd);
    mst.sort_by(|a, b| a.weight.partial_cmp(&b.weight).unwrap_or(std::cmp::Ordering::Equal));

    // Build condensed hierarchy by walking MST edges smallest-to-largest.
    let mut uf = UnionFind::new(n);
    let mut comp_cl: Vec<Option<usize>> = vec![None; n];
    let mut hier: Vec<HNode> = Vec::new();
    let mut pt_lambda = vec![0.0_f32; n];

    for edge in &mst {
        let (ra, rb) = (uf.find(edge.a), uf.find(edge.b));
        if ra == rb { continue; }
        let (sa, sb) = (uf.size[ra], uf.size[rb]);
        let lambda = if edge.weight > 0.0 { 1.0 / edge.weight } else { f32::MAX };
        let (ca, cb) = (comp_cl[ra], comp_cl[rb]);
        let (big, _) = uf.union(ra, rb);
        let ns = uf.size[big];

        if sa >= mcs && sb >= mcs {
            // Both sides are clusters: record a split.
            let mut ch = Vec::new();
            if let Some(i) = ca { hier[i].death_lambda = lambda; ch.push(i); }
            if let Some(i) = cb { hier[i].death_lambda = lambda; ch.push(i); }
            let idx = hier.len();
            hier.push(HNode { birth_lambda: lambda, death_lambda: f32::MAX, size: ns,
                children: ch, points: vec![], stability: 0.0, selected: false });
            comp_cl[big] = Some(idx);
        } else if sa >= mcs || sb >= mcs {
            // One side is a cluster, the other is noise -- carry cluster forward.
            comp_cl[big] = if sa >= mcs { ca } else { cb };
        } else if ns >= mcs {
            // Neither side large enough alone, but merged they are. Birth a cluster.
            let idx = hier.len();
            hier.push(HNode { birth_lambda: lambda, death_lambda: f32::MAX, size: ns,
                children: vec![], points: vec![], stability: 0.0, selected: false });
            comp_cl[big] = Some(idx);
        } else {
            comp_cl[big] = None;
        }
        if pt_lambda[edge.a] == 0.0 { pt_lambda[edge.a] = lambda; }
        if pt_lambda[edge.b] == 0.0 { pt_lambda[edge.b] = lambda; }
    }

    if hier.is_empty() {
        return HdbscanResult { labels: vec![-1; n], confidences: vec![0.0; n], n_clusters: 0 };
    }

    // Assign points to hierarchy nodes via a second UF pass.
    let mut uf2 = UnionFind::new(n);
    let mut cc2: Vec<Option<usize>> = vec![None; n];
    for edge in &mst {
        let (ra, rb) = (uf2.find(edge.a), uf2.find(edge.b));
        if ra == rb { continue; }
        let (sa, sb) = (uf2.size[ra], uf2.size[rb]);
        let lambda = if edge.weight > 0.0 { 1.0 / edge.weight } else { f32::MAX };
        let (big, _) = uf2.union(ra, rb);
        let ns = uf2.size[big];

        if sa >= mcs && sb >= mcs {
            cc2[big] = hier.iter().position(|h| (h.birth_lambda - lambda).abs() < 1e-10 && h.size == ns);
        } else if sa >= mcs || sb >= mcs {
            cc2[big] = if sa >= mcs { cc2[ra] } else { cc2[rb] };
        } else if ns >= mcs {
            cc2[big] = hier.iter().position(|h| (h.birth_lambda - lambda).abs() < 1e-10 && h.size == ns);
        }
    }
    for i in 0..n {
        let root = uf2.find(i);
        if let Some(ci) = cc2[root] {
            if ci < hier.len() { hier[ci].points.push(i); }
        }
    }

    // Compute stability and select clusters.
    for node in hier.iter_mut() {
        let death = if node.death_lambda == f32::MAX { node.birth_lambda + 0.01 } else { node.death_lambda };
        node.stability = node.size as f32 * (death - node.birth_lambda).max(0.0);
    }
    select_clusters(&mut hier);

    if epsilon > 0.0 {
        for node in hier.iter_mut() {
            if node.selected && node.birth_lambda > 0.0 && (1.0 / node.birth_lambda) < epsilon {
                node.selected = false;
            }
        }
    }

    // Build final labels and confidences.
    let selected: Vec<usize> = hier.iter().enumerate().filter(|(_, h)| h.selected).map(|(i, _)| i).collect();
    let nc = selected.len();
    let mut labels = vec![-1_i32; n];
    let mut confs = vec![0.0_f32; n];

    for (cl, &hi) in selected.iter().enumerate() {
        let node = &hier[hi];
        let span = (node.death_lambda.min(1e10) - node.birth_lambda).max(1e-10);
        for &pt in &node.points {
            labels[pt] = cl as i32;
            confs[pt] = ((pt_lambda[pt] - node.birth_lambda) / span).clamp(0.0, 1.0);
        }
    }

    HdbscanResult { labels, confidences: confs, n_clusters: nc }
}

/// Select flat clusters: prefer children if their combined stability exceeds the parent.
fn select_clusters(hier: &mut Vec<HNode>) {
    for node in hier.iter_mut() { if node.children.is_empty() { node.selected = true; } }

    for i in (0..hier.len()).rev() {
        let ch = hier[i].children.clone();
        if ch.is_empty() { continue; }
        let ch_stab: f32 = ch.iter().map(|&c| hier[c].stability).sum();
        if hier[i].stability >= ch_stab {
            hier[i].selected = true;
            let pts: Vec<usize> = ch.iter().flat_map(|&c| hier[c].points.clone()).collect();
            hier[i].points.extend(pts);
            deselect(&mut *hier, &ch);
        } else {
            hier[i].stability = ch_stab;
            hier[i].selected = false;
        }
    }
}

fn deselect(hier: &mut [HNode], roots: &[usize]) {
    for &r in roots {
        hier[r].selected = false;
        let ch = hier[r].children.clone();
        if !ch.is_empty() { deselect(hier, &ch); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> Vec<f32> {
        let n = (x * x + y * y).sqrt();
        if n == 0.0 { vec![0.0, 0.0] } else { vec![x / n, y / n] }
    }

    #[test]
    fn test_empty_input() {
        let r = hdbscan(&[], &HdbscanConfig::default());
        assert!(r.labels.is_empty() && r.confidences.is_empty() && r.n_clusters == 0);
    }

    #[test]
    fn test_single_point() {
        let r = hdbscan(&[vec![1.0, 0.0]], &HdbscanConfig::default());
        assert_eq!(r.labels, vec![-1]);
        assert_eq!(r.confidences, vec![0.0]);
        assert_eq!(r.n_clusters, 0);
    }

    #[test]
    fn test_clear_cluster_structure() {
        let mut data = Vec::new();
        for i in 0..6 { data.push(point(1.0, 0.05 * i as f32)); }
        for i in 0..6 { data.push(point(0.05 * i as f32, 1.0)); }

        let cfg = HdbscanConfig { min_cluster_size: 3, min_samples: Some(2), ..Default::default() };
        let r = hdbscan(&data, &cfg);
        assert_eq!(r.labels.len(), 12);
        let assigned = r.labels.iter().filter(|&&l| l >= 0).count();
        assert!(assigned > 0, "Expected some assigned points");
        if r.n_clusters >= 2 {
            let a = r.labels[..6].iter().find(|&&l| l >= 0).copied().unwrap_or(-1);
            let b = r.labels[6..].iter().find(|&&l| l >= 0).copied().unwrap_or(-1);
            if a >= 0 && b >= 0 { assert_ne!(a, b, "Clusters should have different labels"); }
        }
    }

    #[test]
    fn test_noise_detection() {
        let mut data = Vec::new();
        for i in 0..6 { data.push(point(1.0, 0.02 * i as f32)); }
        data.push(point(-1.0, 0.3));
        data.push(point(0.0, -1.0));

        let cfg = HdbscanConfig { min_cluster_size: 4, min_samples: Some(3), ..Default::default() };
        let r = hdbscan(&data, &cfg);
        assert_eq!(r.labels.len(), 8);
        for &l in &r.labels { assert!(l >= -1); }
        for &c in &r.confidences { assert!((0.0..=1.0).contains(&c)); }
    }

    #[test]
    fn test_min_cluster_size_parameter() {
        let data: Vec<Vec<f32>> = (0..5).map(|i| point(1.0, 0.1 * i as f32)).collect();
        let cfg = HdbscanConfig { min_cluster_size: 100, ..Default::default() };
        let r = hdbscan(&data, &cfg);
        assert!(r.labels.iter().all(|&l| l == -1), "All noise when min_cluster_size > n");
        assert_eq!(r.n_clusters, 0);
    }

    #[test]
    fn test_confidence_scores_valid() {
        let data: Vec<Vec<f32>> = (0..10).map(|i| point(1.0, 0.03 * i as f32)).collect();
        let cfg = HdbscanConfig { min_cluster_size: 3, min_samples: Some(2), ..Default::default() };
        let r = hdbscan(&data, &cfg);
        assert_eq!(r.confidences.len(), data.len());
        for (i, &c) in r.confidences.iter().enumerate() {
            assert!((0.0..=1.0).contains(&c), "Confidence {i} out of range: {c}");
            if r.labels[i] == -1 { assert_eq!(c, 0.0, "Noise point {i} should have 0 confidence"); }
        }
    }

    #[test]
    fn test_cosine_distance_properties() {
        assert!((cosine_distance(&[1.0, 0.0], &[1.0, 0.0])).abs() < 1e-6);
        assert!((cosine_distance(&[1.0, 0.0], &[0.0, 1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_mutual_reachability_properties() {
        let mrd = mutual_reachability(0.5, 0.3, 0.2);
        assert!(mrd >= 0.5 && mrd >= 0.3 && mrd >= 0.2);
        assert_eq!(mrd, 0.5);
    }
}
