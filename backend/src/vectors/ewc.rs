//! Elastic Weight Consolidation (EWC++) for preventing catastrophic forgetting (ADR-004, item #21).
//!
//! When the SONA learning engine updates centroids based on new user feedback,
//! EWC++ applies a regularization penalty that discourages large changes to
//! parameters (centroid dimensions) that were important for previously learned
//! categories. This prevents new feedback from overwriting prior knowledge.
//!
//! The Fisher information matrix diagonal approximation tracks how important
//! each dimension of each centroid is, based on the squared gradients observed
//! during prior updates.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::debug;

use super::types::EmailCategory;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the EWC++ regularizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EwcConfig {
    /// Whether EWC++ regularization is enabled.
    #[serde(default = "default_ewc_enabled")]
    pub enabled: bool,
    /// Regularization strength (lambda). Higher values penalize parameter
    /// changes more aggressively, preserving old knowledge at the cost of
    /// slower adaptation.
    #[serde(default = "default_ewc_lambda")]
    pub lambda: f32,
    /// Decay factor for online Fisher information updates (EWC++ extension).
    /// Controls how quickly old importance estimates fade. Range: 0.0 .. 1.0.
    /// Lower values make the regularizer more adaptive to recent data.
    #[serde(default = "default_ewc_gamma")]
    pub gamma: f32,
    /// Minimum number of updates before EWC penalties activate.
    /// Prevents premature regularization during initial learning.
    #[serde(default = "default_ewc_min_updates")]
    pub min_updates: u32,
}

fn default_ewc_enabled() -> bool {
    true
}
fn default_ewc_lambda() -> f32 {
    0.4
}
fn default_ewc_gamma() -> f32 {
    0.95
}
fn default_ewc_min_updates() -> u32 {
    20
}

impl Default for EwcConfig {
    fn default() -> Self {
        Self {
            enabled: default_ewc_enabled(),
            lambda: default_ewc_lambda(),
            gamma: default_ewc_gamma(),
            min_updates: default_ewc_min_updates(),
        }
    }
}

// ---------------------------------------------------------------------------
// EWC++ Regularizer
// ---------------------------------------------------------------------------

/// Diagonal approximation of the Fisher information matrix for one category.
#[derive(Debug, Clone)]
struct FisherDiag {
    /// Per-dimension importance weights (diagonal of Fisher info matrix).
    importance: Vec<f32>,
    /// The "anchor" parameter values that the regularizer tries to preserve.
    /// Updated periodically via EWC++ online consolidation.
    anchor: Vec<f32>,
    /// Number of updates incorporated into this Fisher estimate.
    update_count: u32,
}

/// EWC++ regularizer that prevents catastrophic forgetting of learned centroids.
///
/// Maintains a per-category Fisher information diagonal and applies an
/// L2 penalty weighted by parameter importance when computing centroid updates.
pub struct EwcRegularizer {
    config: EwcConfig,
    /// Per-category Fisher diagonals.
    fisher: HashMap<EmailCategory, FisherDiag>,
}

impl EwcRegularizer {
    /// Create a new EWC++ regularizer with the given configuration.
    pub fn new(config: EwcConfig) -> Self {
        Self {
            config,
            fisher: HashMap::new(),
        }
    }

    /// Initialize (or reinitialize) the anchor for a category from its current centroid.
    ///
    /// Call this when seeding initial centroids or after a consolidation.
    pub fn initialize_anchor(&mut self, category: EmailCategory, centroid: &[f32]) {
        let dims = centroid.len();
        self.fisher.insert(
            category,
            FisherDiag {
                importance: vec![0.0; dims],
                anchor: centroid.to_vec(),
                update_count: 0,
            },
        );
    }

    /// Record a gradient observation for a category (online Fisher update).
    ///
    /// `gradient` is the per-dimension change that would be applied to the
    /// centroid if there were no regularization. The squared gradient is used
    /// as a Monte Carlo approximation of the Fisher information diagonal.
    ///
    /// EWC++ uses an online moving average:
    ///   F_new = gamma * F_old + (1 - gamma) * gradient^2
    pub fn observe_gradient(&mut self, category: EmailCategory, gradient: &[f32]) {
        if !self.config.enabled {
            return;
        }

        let gamma = self.config.gamma;

        let entry = self.fisher.entry(category).or_insert_with(|| FisherDiag {
            importance: vec![0.0; gradient.len()],
            anchor: vec![0.0; gradient.len()],
            update_count: 0,
        });

        if entry.importance.len() != gradient.len() {
            return;
        }

        for (fi, &gi) in entry.importance.iter_mut().zip(gradient.iter()) {
            *fi = gamma * *fi + (1.0 - gamma) * gi * gi;
        }

        entry.update_count += 1;

        debug!(
            category = %category,
            update_count = entry.update_count,
            "EWC++ Fisher information updated"
        );
    }

    /// Apply EWC++ regularization to a proposed centroid update.
    ///
    /// Given the current centroid and the proposed new centroid (after the
    /// learning rate has been applied), returns a regularized centroid that
    /// penalizes movement in dimensions with high Fisher importance.
    ///
    /// The penalty for dimension i is:
    ///   penalty_i = lambda * F_i * (theta_new_i - theta_anchor_i)^2
    ///
    /// The regularized update blends the proposed update with the anchor:
    ///   theta_reg_i = theta_new_i - lambda * F_i * (theta_new_i - anchor_i)
    ///
    /// If EWC is disabled or insufficient updates have been observed,
    /// returns the proposed centroid unchanged.
    pub fn regularize(
        &self,
        category: EmailCategory,
        _current: &[f32],
        proposed: &[f32],
    ) -> Vec<f32> {
        if !self.config.enabled {
            return proposed.to_vec();
        }

        let fisher_diag = match self.fisher.get(&category) {
            Some(f) => f,
            None => return proposed.to_vec(),
        };

        if fisher_diag.update_count < self.config.min_updates {
            return proposed.to_vec();
        }

        if fisher_diag.anchor.len() != proposed.len() {
            return proposed.to_vec();
        }

        let lambda = self.config.lambda;

        proposed
            .iter()
            .zip(fisher_diag.anchor.iter())
            .zip(fisher_diag.importance.iter())
            .map(|((&p, &a), &f)| {
                // Pull the proposed value back toward the anchor, weighted by importance.
                p - lambda * f * (p - a)
            })
            .collect()
    }

    /// Consolidate the current centroid as the new anchor (EWC++ online).
    ///
    /// Call this after a consolidation cycle (e.g., daily consolidation)
    /// to update the reference point while preserving the Fisher estimates.
    pub fn consolidate_anchor(&mut self, category: EmailCategory, new_centroid: &[f32]) {
        if let Some(entry) = self.fisher.get_mut(&category) {
            if entry.anchor.len() == new_centroid.len() {
                entry.anchor.copy_from_slice(new_centroid);
                debug!(
                    category = %category,
                    "EWC++ anchor consolidated to new centroid"
                );
            }
        }
    }

    /// Compute the total EWC penalty for a category at the proposed parameters.
    ///
    /// penalty = (lambda / 2) * sum_i(F_i * (theta_i - anchor_i)^2)
    pub fn penalty(&self, category: EmailCategory, proposed: &[f32]) -> f32 {
        if !self.config.enabled {
            return 0.0;
        }

        let fisher_diag = match self.fisher.get(&category) {
            Some(f) => f,
            None => return 0.0,
        };

        if fisher_diag.update_count < self.config.min_updates {
            return 0.0;
        }

        let lambda = self.config.lambda;

        let sum: f32 = proposed
            .iter()
            .zip(fisher_diag.anchor.iter())
            .zip(fisher_diag.importance.iter())
            .map(|((&p, &a), &f)| f * (p - a).powi(2))
            .sum();

        (lambda / 2.0) * sum
    }

    /// Return the number of gradient observations for a category.
    pub fn update_count(&self, category: EmailCategory) -> u32 {
        self.fisher
            .get(&category)
            .map(|f| f.update_count)
            .unwrap_or(0)
    }

    /// Return the Fisher importance weights for a category (for diagnostics).
    pub fn importance_weights(&self, category: EmailCategory) -> Option<&[f32]> {
        self.fisher.get(&category).map(|f| f.importance.as_slice())
    }

    /// Whether EWC++ is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(enabled: bool, lambda: f32, gamma: f32, min_updates: u32) -> EwcConfig {
        EwcConfig {
            enabled,
            lambda,
            gamma,
            min_updates,
        }
    }

    #[test]
    fn test_ewc_disabled_returns_proposed_unchanged() {
        let reg = EwcRegularizer::new(make_config(false, 0.5, 0.95, 0));
        let proposed = vec![1.0, 2.0, 3.0];
        let result = reg.regularize(EmailCategory::Work, &[0.0; 3], &proposed);
        assert_eq!(result, proposed);
    }

    #[test]
    fn test_ewc_no_anchor_returns_proposed_unchanged() {
        let reg = EwcRegularizer::new(make_config(true, 0.5, 0.95, 0));
        let proposed = vec![1.0, 2.0, 3.0];
        let result = reg.regularize(EmailCategory::Work, &[0.0; 3], &proposed);
        assert_eq!(result, proposed);
    }

    #[test]
    fn test_ewc_min_updates_guard() {
        let mut reg = EwcRegularizer::new(make_config(true, 0.5, 0.95, 10));
        reg.initialize_anchor(EmailCategory::Work, &[1.0, 0.0, 0.0]);

        // Record 5 gradient observations (below min_updates=10).
        for _ in 0..5 {
            reg.observe_gradient(EmailCategory::Work, &[0.1, 0.2, 0.3]);
        }

        let proposed = vec![2.0, 1.0, 0.5];
        let result = reg.regularize(EmailCategory::Work, &[1.0, 0.0, 0.0], &proposed);
        // Should return proposed unchanged since update_count < min_updates.
        assert_eq!(result, proposed);
    }

    #[test]
    fn test_ewc_regularization_pulls_toward_anchor() {
        let mut reg = EwcRegularizer::new(make_config(true, 0.5, 0.0, 0));

        // Anchor at [1.0, 0.0, 0.0].
        reg.initialize_anchor(EmailCategory::Work, &[1.0, 0.0, 0.0]);

        // Record a gradient that makes dim 0 very important.
        reg.observe_gradient(EmailCategory::Work, &[1.0, 0.0, 0.0]);

        // Propose moving to [2.0, 0.5, 0.5].
        let proposed = vec![2.0, 0.5, 0.5];
        let result = reg.regularize(EmailCategory::Work, &[1.0, 0.0, 0.0], &proposed);

        // Dim 0 has high importance (F=1.0), so it should be pulled back.
        // result[0] = 2.0 - 0.5 * 1.0 * (2.0 - 1.0) = 2.0 - 0.5 = 1.5
        assert!(
            (result[0] - 1.5).abs() < 1e-5,
            "Expected 1.5, got {}",
            result[0]
        );
        // Dim 1 has zero importance, so no pull.
        // result[1] = 0.5 - 0.5 * 0.0 * (0.5 - 0.0) = 0.5
        assert!(
            (result[1] - 0.5).abs() < 1e-5,
            "Expected 0.5, got {}",
            result[1]
        );
        // Dim 2 also zero importance.
        assert!(
            (result[2] - 0.5).abs() < 1e-5,
            "Expected 0.5, got {}",
            result[2]
        );
    }

    #[test]
    fn test_ewc_online_fisher_update() {
        let mut reg = EwcRegularizer::new(make_config(true, 0.5, 0.9, 0));
        reg.initialize_anchor(EmailCategory::Work, &[0.0, 0.0]);

        // First gradient: [1.0, 0.0]
        reg.observe_gradient(EmailCategory::Work, &[1.0, 0.0]);
        // F = 0.9 * [0,0] + 0.1 * [1,0] = [0.1, 0.0]
        let w = reg.importance_weights(EmailCategory::Work).unwrap();
        assert!((w[0] - 0.1).abs() < 1e-5);
        assert!((w[1] - 0.0).abs() < 1e-5);

        // Second gradient: [0.0, 2.0]
        reg.observe_gradient(EmailCategory::Work, &[0.0, 2.0]);
        // F = 0.9 * [0.1, 0.0] + 0.1 * [0.0, 4.0] = [0.09, 0.4]
        let w = reg.importance_weights(EmailCategory::Work).unwrap();
        assert!((w[0] - 0.09).abs() < 1e-5);
        assert!((w[1] - 0.4).abs() < 1e-5);
    }

    #[test]
    fn test_ewc_penalty_computation() {
        let mut reg = EwcRegularizer::new(make_config(true, 1.0, 0.0, 0));
        reg.initialize_anchor(EmailCategory::Work, &[0.0, 0.0]);

        // Observe gradient to set Fisher info.
        reg.observe_gradient(EmailCategory::Work, &[1.0, 1.0]);
        // F = [1.0, 1.0]

        // Penalty at proposed = [1.0, 1.0]:
        // (1.0/2) * (1.0 * 1.0^2 + 1.0 * 1.0^2) = 0.5 * 2.0 = 1.0
        let penalty = reg.penalty(EmailCategory::Work, &[1.0, 1.0]);
        assert!(
            (penalty - 1.0).abs() < 1e-5,
            "Expected 1.0, got {}",
            penalty
        );
    }

    #[test]
    fn test_ewc_consolidate_anchor() {
        let mut reg = EwcRegularizer::new(make_config(true, 0.5, 0.0, 0));
        reg.initialize_anchor(EmailCategory::Work, &[1.0, 0.0]);
        reg.observe_gradient(EmailCategory::Work, &[1.0, 0.0]);

        // Consolidate with new centroid.
        reg.consolidate_anchor(EmailCategory::Work, &[2.0, 1.0]);

        // Now proposed = [2.0, 1.0] should have zero penalty (at anchor).
        let penalty = reg.penalty(EmailCategory::Work, &[2.0, 1.0]);
        assert!(
            penalty < 1e-5,
            "Expected near-zero penalty, got {}",
            penalty
        );
    }

    #[test]
    fn test_ewc_default_config() {
        let config = EwcConfig::default();
        assert!(config.enabled);
        assert!((config.lambda - 0.4).abs() < 1e-5);
        assert!((config.gamma - 0.95).abs() < 1e-5);
        assert_eq!(config.min_updates, 20);
    }

    #[test]
    fn test_ewc_update_count() {
        let mut reg = EwcRegularizer::new(make_config(true, 0.5, 0.95, 0));
        assert_eq!(reg.update_count(EmailCategory::Work), 0);

        reg.initialize_anchor(EmailCategory::Work, &[0.0, 0.0]);
        assert_eq!(reg.update_count(EmailCategory::Work), 0);

        reg.observe_gradient(EmailCategory::Work, &[1.0, 0.0]);
        assert_eq!(reg.update_count(EmailCategory::Work), 1);

        reg.observe_gradient(EmailCategory::Work, &[0.0, 1.0]);
        assert_eq!(reg.update_count(EmailCategory::Work), 2);
    }
}
