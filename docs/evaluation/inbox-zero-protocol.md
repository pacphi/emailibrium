# "10-Minute Inbox Zero" Validation Protocol

## Objective

Validate the claim that users can achieve inbox zero for 10,000+ emails within 10 minutes using Emailibrium (docs/plan/inception.md Section 2.2, docs/research/initial.md Section 6.4).

## Study Design

### Participants

- N >= 30 heavy email users (>100 emails/day)
- Recruited from tech professionals (mix of roles: engineering, product, support, sales)
- Must have at least one email account with 5,000+ emails in the inbox
- Exclusion criteria: prior experience with vector-based email tools, email bankruptcy practitioners

### Pre-Study Questionnaire

1. Average daily email volume
2. Current time spent managing email (minutes/day)
3. Number of unread emails in primary inbox
4. Email clients used (Gmail web, Outlook, Apple Mail, etc.)
5. Self-rated email management skill (1-5 Likert scale)

### Protocol

#### Phase 0: Setup (not timed)

- Participant signs informed consent
- Research assistant verifies inbox size >= 5,000 emails
- System requirements check (browser version, network speed)

#### Phase 1: Baseline Measurement (5 min)

1. Participant attempts to organize inbox using their current email client
2. Record: number of emails triaged, actions taken, strategy used
3. Timer stops at 5 minutes
4. Capture screenshot of inbox state

#### Phase 2: Connection (2 min)

1. Connect email account to Emailibrium via OAuth
2. Verify successful connection
3. Note any connection issues

#### Phase 3: Ingestion (measured, not counted in triage time)

1. System processes all emails
2. Record: total ingestion time, emails/second throughput
3. Record: time to first usable result (streaming threshold)
4. Record: peak memory usage during ingestion

#### Phase 4: Triage (timed -- primary metric)

1. Participant uses Inbox Cleaner to reach inbox zero
2. Timer starts when participant clicks "Start Analysis"
3. Timer stops when last action is taken (inbox empty or all items triaged)
4. Record: every click/tap via event tracking
5. Record: which features were used (batch actions, categories, search)

#### Phase 5: Satisfaction Survey

1. System Usability Scale (SUS) -- standard 10-item questionnaire
2. Custom questions (see below)
3. Open-ended feedback

### Custom Survey Questions

| #   | Question                                                 | Scale      |
| --- | -------------------------------------------------------- | ---------- |
| Q1  | I felt confident in the system's email categorization    | 1-5 Likert |
| Q2  | The batch actions saved me significant time              | 1-5 Likert |
| Q3  | I would trust this system with my daily email management | 1-5 Likert |
| Q4  | The semantic search found emails I could not find before | 1-5 Likert |
| Q5  | I would recommend this tool to a colleague               | 1-5 Likert |

### Follow-Up (1 week)

1. Measure inbox rebound: how many emails accumulated without being triaged
2. Repeat SUS questionnaire
3. Record: daily usage time, features used

## Metrics

| Metric                  | Target           | How Measured                                          |
| ----------------------- | ---------------- | ----------------------------------------------------- |
| Time to inbox zero      | < 10 min         | Stopwatch from "Start Analysis" to last action        |
| User actions required   | < 50             | Click/tap count via event tracking                    |
| Ingestion throughput    | > 500 emails/sec | Server metrics (emails processed / elapsed time)      |
| User satisfaction       | SUS > 70         | Post-task questionnaire                               |
| 1-week inbox rebound    | < 100 emails     | Follow-up measurement                                 |
| Categorization accuracy | > 95%            | Manual spot-check of 20 random emails per participant |
| Search latency (p95)    | < 50ms           | Server-side instrumentation                           |
| Error rate              | < 1%             | Count of failed actions / total actions               |

## Control Conditions

### Condition A: Current Email Client (within-subjects)

- Same participant, same inbox, existing email client
- 5-minute timed triage session
- Measures baseline organizational speed
- Order counterbalanced: half of participants do Condition A first

### Condition B: Emailibrium Without Vector Intelligence

- Emailibrium UI with keyword search only (vector features disabled)
- Same triage task as the experimental condition
- Isolates the value added by semantic/vector intelligence
- Useful for attributing improvement to vector tech vs. better UI

### Condition C: Full Emailibrium (experimental)

- All features enabled: semantic search, vector classification, batch actions, insights
- Primary experimental condition

## Statistical Analysis

### Primary Analysis

- Paired t-test (or Wilcoxon signed-rank if normality violated) for time comparison between conditions
- Effect size: Cohen's d
- Minimum detectable effect: 5 minutes (half the target)
- Alpha: 0.05, Power: 0.80
- Required sample size: N >= 27 (computed via G\*Power for paired t-test with d=0.5); N=30 accounts for dropout

### Secondary Analyses

- ANOVA across all three conditions with Bonferroni correction
- Regression: time-to-inbox-zero ~ inbox_size + daily_volume + self_rated_skill
- Correlation: SUS score vs. time-to-inbox-zero
- Sub-group analysis: heavy users (>200 emails/day) vs. moderate users

### Data Quality

- Inter-rater reliability for manual categorization accuracy check (Cohen's kappa)
- Outlier detection: participants who did not follow instructions (e.g., deleted all emails without reading)
- Missing data handling: last observation carried forward for dropout participants

## Ethical Considerations

- Informed consent required before participation
- Email data processed locally only -- never transmitted to research servers
- Participants can withdraw at any time without penalty
- No email content shared with researchers; only aggregate metrics collected
- Study protocol reviewed by institutional ethics board (or equivalent)
- Data retention: anonymized metrics kept for 2 years; raw logs deleted after analysis
- Compensation: participants receive a summary of their inbox health report

## Technical Requirements

### Test Environment

- Dedicated test server with reproducible configuration
- Network latency monitoring throughout sessions
- Browser: latest Chrome or Firefox (controlled variable)
- Minimum hardware: 8GB RAM, modern multi-core CPU

### Instrumentation

- Frontend event tracking: all clicks, keypresses, navigation events
- Backend metrics: API latency, throughput, error rates
- System metrics: CPU, memory, disk I/O during ingestion
- All timestamps synchronized via NTP

### Data Collection

- All metrics stored in a separate analytics database (not the production email database)
- Participant IDs are pseudonymized (UUID, no PII in analytics)
- Raw event logs encrypted at rest

## Reporting

### Expected Deliverables

1. Statistical report with primary and secondary analysis results
2. Participant experience summary (anonymized quotes, common themes)
3. Technical performance report (throughput, latency percentiles)
4. Recommendations for product improvement based on observed pain points

### Publication Plan

- Internal report: within 2 weeks of study completion
- Blog post: anonymized summary of key findings
- Academic paper (optional): if results are statistically significant and novel
