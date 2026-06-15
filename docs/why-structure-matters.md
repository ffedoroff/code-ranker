# Why structure matters — the evidence behind code-ranker

This document is the **evidence base and rationale** for the signals code-ranker
measures. The product's premise is simple:

> Structural badness is not an aesthetic problem. It is **compound interest on
> speed and risk** — the longer it is ignored, the more every future change costs
> and the more defects accumulate. code-ranker measures the structural signals
> (dependency cycles, coupling/Henry-Kafura, complexity, size) that **predict
> where it will hurt**, so a team can pay the debt down deliberately and early
> instead of paying for a crisis later.

Below is the empirical support for that premise — peer-reviewed studies,
foundational books, and industry research — organized by the three signal
families code-ranker reports. It is written to be **honest**: most of the
evidence is observational/correlational, and there is a real, well-documented
**size-confound** critique that we present at full strength (§4) because it
directly shapes how the tool's own numbers should be read.

A note on verification: figures below were checked against primary sources
(DOIs, arXiv PDFs, original reports). Anything that could not be confirmed from
the primary text is explicitly marked **[unverified]** so it is not quoted as
settled fact.

---

## TL;DR — what the literature actually supports

1. **Broadly confirmed (1981 → 2022, across ecosystems):** code in dependency
   cycles, code with high information-flow coupling, and large/complex modules
   *co-occur* with more defects and more change effort. The breadth and
   consistency of this finding matters more than any single number.
2. **Cycles and coupling *as a class* are the strongest foundation** for the
   product. Note an important nuance (§2): the broad *idea* that information-flow
   coupling tracks faults/effort is well supported, but the **specific
   Henry-Kafura `length × (fan_in × fan_out)²` formula is *not* independently
   validated** — independent tests criticized it (beaten by LOC; arbitrary
   squaring; reads zero when fan-in or fan-out is zero). Treat HK as a cheap
   ranking heuristic, not a validated law.
3. **The size-confound is real and must be disclosed:** at the **file** level,
   cyclomatic complexity is largely redundant with lines of code. The
   independent signal lives in **cycles, coupling, and nesting-aware cognitive
   complexity**, and at **method** granularity — not in file-level cyclomatic
   complexity "on its own."
4. **No universal threshold generalizes across projects** (Microsoft study,
   §3) — which is *why* code-ranker calibrates thresholds per project and emits a
   **shortlist for human review**, not an automated verdict.
5. **Causation is not established.** These are leading indicators / proxies, not
   proof. We say so plainly, and the product reflects it (human-in-the-loop).

---

## 1. Dependency cycles (ADP) → more defects, more change

The Acyclic Dependencies Principle is the highest-priority signal code-ranker
surfaces. The evidence that cyclic code is worse is specific and quantified.

### Oyetoyan, Cruzes & Conradi (2013) — defect profile

Components inside dependency cycles are markedly more defect-prone. At **package
level**, the share of components that are defective (cyclic vs non-cyclic):
ActiveMQ **45% vs 12%**, Camel **31% vs 11%**, openPDC **15% vs 1%**, Eclipse
**>50% vs ~30%**. For Camel, **"90% of all the defective classes are in the
in-cycle group."** Effect sizes (Hedges' g) range up to ~4.6; p-values mostly
< 0.0001.
- *Method:* 6 systems — Apache Camel, ActiveMQ, Lucene, Eclipse (Java) +
  commApp & openPDC (industrial C# smart-grid), ~800–12,700 classes each.
- **Caveat (their own data):** cyclic groups are disproportionately *large*
  (e.g. ActiveMQ in-cycle = 32.5% of classes but 55% of code size), so the
  size-confound (§4) is present; openPDC is a **negative case** at class level
  (not significant). No odds ratios reported. Observational, not causal.
- Citation: T. D. Oyetoyan, D. S. Cruzes, R. Conradi. "A study of cyclic
  dependencies on defect profile of software components." *Journal of Systems
  and Software* 86(12):3162–3182, 2013.
  DOI: <https://doi.org/10.1016/j.jss.2013.07.039>

### Oyetoyan, Falleri, Dietrich & Jezek (2015) — change-proneness

12 Java systems, 389 versions. The pure in-cycle effect was significant in only
2/12 systems — **but** including a cycle's *direct neighbourhood*, 75% of systems
showed significant change-proneness, and **"SCCs and their direct neighbours
account for more than 90% of the total change"** (e.g. Apache Ant: those classes
= 76.3% of classes but **94% of total change volume**). Classes in
strongly-connected components ranged from 10.3% to 80.7% of all classes
(freecol 80.7%, hibernate 62.8%).
- **Caveat:** the harmful signal is really "cycle **plus** its neighbourhood,"
  not the SCC alone; single ecosystem (Java OSS); no standardized effect size.
- Citation: T. D. Oyetoyan, J.-R. Falleri, J. Dietrich, K. Jezek. "Circular
  dependencies and change-proneness: An empirical study." *SANER 2015*,
  pp. 241–250. DOI: <https://doi.org/10.1109/SANER.2015.7081834>

### Melton & Tempero (2007) — prevalence (context, not harm)

78 Java applications: ~45% contained a cycle of ≥100 classes; ~10% a cycle of
≥1,000 classes. Establishes that large cycles are *common*; **does not** itself
show harm.
- Citation: H. Melton, E. Tempero. "An empirical study of cycles among classes
  in Java." *Empirical Software Engineering* 12(4):389–415, 2007.
  DOI: <https://doi.org/10.1007/s10664-006-9033-1>

### Robert C. Martin — the principle (rationale, not statistics)

The canonical statement of *why* cycles are bad: **"Allow no cycles in the
component dependency graph."** Cycles cause the "morning-after syndrome" and
force otherwise-independent components into a shared build / test / release
cadence.
- Citation: R. C. Martin. *Clean Architecture: A Craftsman's Guide to Software
  Structure and Design.* Prentice Hall, 2017 — Ch. 14, "Component Coupling."
  (Also *Agile Software Development, Principles, Patterns, and Practices*, 2002.)

---

## 2. Coupling / Henry-Kafura → defects and change effort

code-ranker's headline coupling metric is **HK = `sloc × (fan_in × fan_out)²`**.
This is, almost verbatim, the Henry-Kafura information-flow metric — so the
original validation of that metric is unusually direct evidence for the tool.

### Henry & Kafura (1981) — the original fan-in/fan-out paper

Procedure complexity defined as `length × (fan_in × fan_out)²`, validated against
the UNIX (V6) operating system cross-referenced with 80 changes:
- complexity vs number of changes: **Spearman r = 0.94**;
- the `(fan_in × fan_out)²` term **alone: r = 0.98**;
- **procedure length alone: only r = 0.60.**
- "Eleven out of the twelve procedures with [highest] complexity required
  changes."

The key takeaway: the **coupling term, not raw size, drove the correlation** —
the single best counter to "it's just size" for this exact metric.
- **Caveat:** single small system; author-collected change data; and the formula
  (especially the squaring) was later criticized methodologically — M. Shepperd,
  D. Ince, "A critique of three metrics," *Journal of Systems and Software*
  26(3):197–210, 1994. DOI: <https://doi.org/10.1016/0164-1212(94)90011-6>
- Citation: S. Henry, D. Kafura. "Software Structure Metrics Based on
  Information Flow." *IEEE TSE* SE-7(5):510–518, 1981.
  DOI: <https://doi.org/10.1109/TSE.1981.231113>

### Independent research on the HK formula — a candid caveat

The 1981 numbers above are the metric authors' **own** self-report on a single
system. The honest state of the *independent* literature is uncomfortable: there
is **essentially no independent study validating the exact
`length × (fan_in × fan_out)²` formula**, and the studies that tested it most
directly **criticized** it. Only the weaker underlying idea — that fan-in/fan-out
coupling relates to faults and effort — gets qualified support, and usually via a
*different* formula.

- **Kitchenham, Pickard & Linkman (1990)** — the strongest direct independent
  test, on a real communications system. From the abstract: informational
  **fan-out** could identify change-/fault-prone and complex programs, **but code
  metrics (lines of code and number of branches) were better**; **fan-in was not
  related** to the quality characteristics. I.e. the HK-derived metric offered no
  advantage over trivial size counts.
  Citation: B. A. Kitchenham, L. M. Pickard, S. J. Linkman. "An evaluation of
  some design metrics." *Software Engineering Journal* 5(1):50–58, 1990.
  DOI: <https://doi.org/10.1049/sej.1990.0007>
- **Shepperd & Ince (1994)** — subjected Halstead, McCabe, and Henry-Kafura to
  in-depth critique; all "found wanting." For HK specifically: because the metric
  is multiplicative, **any module with `fan_in = 0` OR `fan_out = 0` collapses to
  complexity zero** regardless of size, and the squaring of `(fan_in × fan_out)`
  is measurement-theoretically arbitrary. Shepperd's own refinement discards the
  `length` factor — he did not accept the original formula. *(The verbatim
  sentences inside the paper are [unverified] — paywalled; the zero-collapse flaw
  is corroborated by S. H. Kan, *Metrics and Models in Software Quality
  Engineering*.)*
  Citation: M. Shepperd, D. C. Ince. "A critique of three metrics." *Journal of
  Systems and Software* 26(3):197–210, 1994.
  DOI: <https://doi.org/10.1016/0164-1212(94)90011-6> · also M. Shepperd, "Design
  metrics: an empirical analysis." *Software Engineering Journal* 5(1):3–10, 1990.
  DOI: <https://doi.org/10.1049/sej.1990.0002>
- **Card & Agresti (1988) / Card & Glass (1990)** — independent NASA/SEL work
  *does* support the broad idea, but with a **different model**: structural
  complexity = `mean(fan_out²)` (**fan-out only, no fan-in, additive** with a
  data-complexity term) — not HK's single multiplicative product. Card & Glass
  report, for n = 8 projects, a correlation between system complexity and defect
  rate of **r ≈ 0.83** (R² ≈ 0.69) *(figures secondary-sourced via Kan; small
  sample)*. This validates fan-out-based design complexity, **not**
  `(fan_in × fan_out)²`.
  Citations: D. N. Card, W. W. Agresti. "Measuring software design complexity."
  *Journal of Systems and Software* 8(3):185–197, 1988.
  DOI: <https://doi.org/10.1016/0164-1212(88)90021-0> · D. N. Card, R. L. Glass.
  *Measuring Software Design Quality.* Prentice Hall, 1990.
- **Modern fault-prediction literature** — the HK information-flow metric is
  effectively **absent**. The major systematic review, **Radjenović, Heričko,
  Torkar & Radjenović (2013)** (106 studies), contains **zero** mentions of
  Henry, Kafura, information flow, or fan-in/fan-out; the field is dominated by
  CK object-oriented metrics, McCabe, and LOC.
  Citation: D. Radjenović et al. "Software fault prediction metrics: A systematic
  literature review." *Information and Software Technology* 55(8):1397–1418, 2013.
  DOI: <https://doi.org/10.1016/j.infsof.2013.02.009>
- **Not independent:** Kafura & Reddy (1987), *IEEE TSE* SE-13(3):335–343
  (DOI: <https://doi.org/10.1109/TSE.1987.233164>) is co-authored by Kafura and is
  a *qualitative* concurrence with expert judgment — not independent corroboration.

**Net for HK:** the *direction* (high information-flow coupling ↔ more
faults/effort) has decades of broad support; the **specific
`length × (fan_in × fan_out)²` formula does not** — it is beaten by LOC/branch
counts in the one strong independent test, its squaring is arbitrary, and it
**reads zero whenever fan-in or fan-out is zero** (so it is silent on pure
sources and sinks — a real limitation of the value code-ranker computes). Lean on
cycles and on coupling *as a class* (CBO, propagation cost, DORA); treat HK as a
useful, cheap ranking heuristic, not an independently validated law.

### Subramanyam & Krishnan (2003) — coupling survives a size control

On a commercial e-commerce system (C++ and Java), the abstract states: **"even
after controlling for the size of the software, these metrics [WMC, CBO] are
significantly associated with defects."** This is the cleanest "not just size"
result for coupling.
- **Caveat:** the effect direction/strength **differed between C++ and Java** —
  not uniformly robust; exact coefficients [unverified] (paywalled).
- Citation: R. Subramanyam, M. S. Krishnan. "Empirical Analysis of CK Metrics
  for Object-Oriented Design Complexity: Implications for Software Defects."
  *IEEE TSE* 29(4):297–310, 2003.
  DOI: <https://doi.org/10.1109/TSE.2003.1191795>

### Basili, Briand & Melo (1996) — CK metrics predict faults

Coupling (CBO) was a significant fault predictor (univariate p ≈ 0.0000); 5 of 6
CK metrics useful; CK metrics outperformed traditional code metrics.
- **Caveat:** 180 C++ classes built by **student teams**; small scale; **did not
  control for size**.
- Citation: V. R. Basili, L. C. Briand, W. L. Melo. "A Validation of
  Object-Oriented Design Metrics as Quality Indicators." *IEEE TSE*
  22(10):751–761, 1996. DOI: <https://doi.org/10.1109/32.544352> ·
  PDF: <http://www.cs.umd.edu/~basili/publications/journals/J62.pdf>

### Gyimóthy, Ferenc & Siket (2005) — CBO best on open source

On Mozilla (7 versions, faults from Bugzilla): **"CBO seems to be the best in
predicting the fault-proneness of classes,"** with LOC second-best.
- **Caveat:** authors note "the precision of our models is not yet
  satisfactory"; CBO and LOC came out essentially tied (consistent with the
  size confound); exact precision/recall figures [unverified].
- Citation: T. Gyimóthy, R. Ferenc, I. Siket. "Empirical Validation of
  Object-Oriented Metrics on Open Source Software for Fault Prediction." *IEEE
  TSE* 31(10):897–910, 2005. DOI: <https://doi.org/10.1109/TSE.2005.112>

### MacCormack, Rusnak & Baldwin (2006) — architecture-level coupling

Defines **"propagation cost"** = average % of system files potentially affected
by a change to a random file (via transitive dependency). **Mozilla 17.35% vs
Linux 5.16%.** After Mozilla's modular redesign, propagation cost dropped **from
17.35% to 2.78%** — "changes to a source file have the potential to impact 80%
fewer source files."
- **Caveat:** two-system comparison, exploratory; propagation cost is a
  structural proxy, not measured dollars/defects.
- Citation: A. MacCormack, J. Rusnak, C. Y. Baldwin. "Exploring the Structure of
  Complex Software Designs: An Empirical Study of Open Source and Proprietary
  Code." *Management Science* 52(7):1015–1030, 2006.
  DOI: <https://doi.org/10.1287/mnsc.1060.0552> ·
  WP PDF: <https://www.hbs.edu/ris/Publication%20Files/05-016.pdf>

### Sturtevant (2013) — architectural complexity has business teeth

Statistical study of a large commercial codebase (coupling via DSM "visibility"
and cyclomatic complexity): differences in architectural complexity **"could
account for 50% drops in productivity, three-fold increases in defect density,
and order-of-magnitude increases in staff turnover."** Tightly-coupled "Core"
files cost more defect-related activity than loosely-coupled "Peripheral" files.
- **Caveat:** observational ("could account for"); largely a single firm. Attribute
  the 3× / 50% / 10× figures to the 2013 thesis (the precise multiplier in the
  2016 journal version is [unverified]).
- Citations: D. J. Sturtevant. "System Design and the Cost of Architectural
  Complexity." PhD thesis, MIT, 2013.
  <https://dspace.mit.edu/handle/1721.1/79551> · and A. MacCormack, D. J.
  Sturtevant. "Technical debt and system architecture: The impact of coupling on
  defect-related activity." *Journal of Systems and Software* 120:170–182, 2016.
  DOI: <https://doi.org/10.1016/j.jss.2016.06.007>

### Forsgren, Humble & Kim — *Accelerate* / DORA

A **loosely coupled architecture** (teams can make large changes and deploy
independently, without cross-team coordination) is among the strongest
predictors of software-delivery performance. The DORA 2021 report found
high/elite performers were **~3× more likely** to have a loosely coupled
architecture.
- **Caveat:** self-reported survey data (~23,000+ cumulative responses),
  cross-sectional/correlational; the "~3×" figure is from the **2021** DORA
  report, not the 2018 book.
- Citations: N. Forsgren, J. Humble, G. Kim. *Accelerate: The Science of Lean
  Software and DevOps.* IT Revolution Press, 2018. · DORA, "State of DevOps"
  reports. <https://dora.dev/capabilities/loosely-coupled-teams/>

### Zimmermann & Nagappan (2008) — dependency-network metrics (context)

On Windows Server 2003, network-analysis measures on the dependency graph
identified **60% of the binaries developers considered critical — twice as many**
as complexity metrics alone, with recall ~10 points higher.
- **Caveat:** about dependency-graph *network* metrics broadly, **not cycles
  specifically**; binary count [unverified].
- Citation: T. Zimmermann, N. Nagappan. "Predicting Defects using Network
  Analysis on Dependency Graphs." *ICSE 2008*, pp. 531–540.
  DOI: <https://doi.org/10.1145/1368088.1368161>

---

## 3. Complexity and size → defects (with the central caveat)

### Evidence that complexity predicts defects

- **Basili, Briand & Melo (1996)** — see §2; complexity-family CK metrics
  outperformed traditional code metrics.
- **Khoshgoftaar et al. (1996)** — discriminant analysis on static
  complexity metrics classified fault-prone modules in a large legacy telecom
  system. *Exact misclassification percentages [unverified] (secondary source).*
  Citation: T. M. Khoshgoftaar, E. B. Allen, K. S. Kalaichelvan, N. Goel. "Early
  Quality Prediction: A Case Study in Telecommunications." *IEEE Software*
  13(1):65–71, 1996. DOI: <https://doi.org/10.1109/52.476287>
- **Nagappan, Ball & Zeller (2006)** — five Microsoft systems; the crucial honest
  nuance, verbatim: "failure-prone software entities are statistically correlated
  with code complexity measures. **However, there is no single set of complexity
  metrics that could act as a universally best defect predictor.**" Predictors
  transfer only to *similar* projects → **calibrate per project, don't claim
  universal thresholds.**
  Citation: N. Nagappan, T. Ball, A. Zeller. "Mining Metrics to Predict Component
  Failures." *ICSE 2006*, pp. 452–461.
  DOI: <https://doi.org/10.1145/1134285.1134349> ·
  <https://www.st.cs.uni-saarland.de/publications/details/nagappan-icse-2006/>

### Metric definitions code-ranker relies on

- **Cyclomatic complexity** — T. J. McCabe. "A Complexity Measure." *IEEE TSE*
  SE-2(4):308–320, 1976. DOI: <https://doi.org/10.1109/TSE.1976.233837>
  **Correction to common folklore:** the famous "the upper bound … is 10 which
  seems like a reasonable, but not magical, upper limit" line is from **NIST
  SP 500-235** (Watson & McCabe, 1996), *not* the 1976 paper.
  <https://www.nist.gov/publications/structured-testing-testing-methodology-using-cyclomatic-complexity-metric>
- **Cognitive complexity** — G. A. Campbell. "Cognitive Complexity: An Origin
  Story, Overview, and Evaluation." *TechDebt 2018*, ACM.
  DOI: <https://doi.org/10.1145/3194164.3194186> · white paper:
  <https://www.sonarsource.com/resources/cognitive-complexity/>
  Designed to fix cyclomatic's blind spot for *understandability*: it ignores
  structures that collapse many statements into one (a whole `switch` counts
  once), increments per break in linear flow, and adds a **nesting penalty**.
  *Caveat: vendor-authored; the evaluation is SonarSource's own argument.*
- **Halstead metrics** — M. H. Halstead. *Elements of Software Science.* Elsevier
  North-Holland, 1977. **Heavily contested:** R. Hamer, G. Frewin, "M.H.
  Halstead's Software Science — A Critical Examination," *ICSE 1982*, found the
  derived Difficulty/Effort/bug relationships "represent neither natural laws nor
  useful engineering approximations." Volume/Length survive as size proxies; the
  derived bug/effort numbers are the weakest.
  <https://dl.acm.org/doi/10.5555/800254.807762>
- **Maintainability Index** — P. Oman, J. Hagemeister, "Construction and testing
  of polynomials predicting software maintainability," *JSS* 24(3):251–266, 1994.
  DOI: <https://doi.org/10.1016/0164-1212(94)90067-1>
  MI is literally built from Halstead Volume + cyclomatic + LOC + comment %.
  **Cite with heavy caveats:** the constants come from a tiny late-1980s HP
  dataset (1,000–10,000 LOC systems; "no statistical significance was
  reported"), the comment term is gameable, and every sub-metric correlates with
  size. See A. van Deursen, "Think Twice Before Using the Maintainability Index"
  (2014): <https://avandeursen.com/2014/08/29/think-twice-before-using-the-maintainability-index/>

---

## 4. The size-confound critique (disclosed at full strength)

This is the most important honesty point, and it bears **directly** on how
code-ranker's own per-file numbers should be read.

- **Jay et al. (2009)** — the largest study (~1.2 million source files). At the
  file level, cyclomatic complexity and LOC correlate with **Pearson r up to
  0.98** (robust models R² up to 0.97). Verbatim conclusion: **"CC has no
  explanatory power of its own and … LOC and CC measure the same property."**
  *Caveat: analysis in log-space; redundancy holds "only in aggregate."*
  Citation: G. Jay et al. "Cyclomatic Complexity and Lines of Code: Empirical
  Evidence of a Stable Linear Relationship." *JSEA* 2(3):137–143, 2009.
  DOI: <https://doi.org/10.4236/jsea.2009.23020> ·
  <https://www.scirp.org/journal/paperinformation?paperid=779>
- **El Emam et al. (2001)** — in a large C++ telecom framework, most OO metrics
  (including coupling measures like CBO) **lose their association with
  fault-proneness once class size is controlled for.** *Exact "survivor count"
  varies across summaries — direction (size is a confounder) is not disputed; the
  specific count is [unverified] and the result was formally contested in IEEE
  TSE.* Citation: K. El Emam, S. Benlarbi, N. Goel, S. N. Rai. "The Confounding
  Effect of Class Size on the Validity of Object-Oriented Metrics." *IEEE TSE*
  27(7):630–650, 2001. DOI: <https://doi.org/10.1109/32.935855>
- **Fenton & Neil (1999)** — identify **multicollinearity** as "the most common
  methodological problem": complexity metrics are so correlated with size that
  complexity's independent contribution cannot be isolated.
  Citation: N. E. Fenton, M. Neil. "A Critique of Software Defect Prediction
  Models." *IEEE TSE* 25(5):675–689, 1999.
  DOI: <https://doi.org/10.1109/32.815326>
- **Gil & Lalouche (2017)** — a metric's apparent fault-correlation validity is
  **largely explained by its correlation with size** (predictable with R² as high
  as ~0.97 [lightly verified]). Citation: Y. Gil, G. Lalouche. "On the
  correlation between size and metric validity." *Empirical Software Engineering*
  22(5):2585–2611, 2017. DOI: <https://doi.org/10.1007/s10664-017-9513-5>

### The counter to the counter — where complexity *does* add signal

- **Landman et al. (2016)** — at **method / function** granularity (17.6M Java
  methods, 6.3M C functions), CC and LOC do **not** strongly correlate. The
  strong file-level correlation in Jay et al. is partly an **aggregation
  artifact**. So the redundancy verdict is *granularity-dependent*.
  Citation: D. Landman, A. Serebrenik, E. Bouwers, J. J. Vinju. "Empirical
  analysis of the relationship between CC and SLOC in a large corpus of Java
  methods and C functions." *Journal of Software: Evolution and Process*
  28(7):589–618, 2016.
  Preprint: <https://aserebre.win.tue.nl/Landman2015-ccsloc-jsep2015-preprint.pdf>

**Implication for code-ranker:** because the tool reports metrics **per file**,
its `cyclomatic` and `loc` columns will be **highly correlated** — file-level
cyclomatic complexity carries little signal "on its own." The independent,
defensible signal lives in: **dependency cycles, Henry-Kafura coupling, and
nesting-aware cognitive complexity** (and would live in per-function analysis if
added). The product should lean on those and be candid about the rest.

---

## 5. The business cost (for non-technical stakeholders)

### Tornhill & Borg (2022) — "Code Red" (the headline study)

39 proprietary production codebases, 30,737 source files, 14 languages.
Comparing low- vs high-quality code (by CodeScene's "Code Health" score):
- **15× more defects** — avg Jira defects/file 3.70 (Alert) vs 0.25 (Healthy);
  Cohen's d = 0.73 ("medium-to-large").
- **+124% more time** to resolve issues in low-quality code — from *normalized*
  values; the raw-minute ratio ≈ 2.24×; d = 0.45 (the **weakest** effect).
- **9× longer maximum cycle time** — 129,940 vs 15,112 min; d = 0.96 (the
  **strongest, most robust** finding; "uncertainty" / variance, not the mean).
- **Caveats — state these openly:** correlational, not causal (the authors ask
  the chicken-or-egg question themselves); **vendor study** on the vendor's own
  customers using the vendor's proprietary metric (lead author is CodeScene's
  CTO); severe class imbalance (only 1.1% "Alert" files); headline ratios are
  Alert-vs-Healthy extremes. The "42% of developers' time wasted" line often
  associated with it is **Stripe's** figure, not Code Red's.
- Citation: A. Tornhill, M. Borg. "Code Red: The Business Impact of Code
  Quality — A Quantitative Study of 39 Proprietary Production Codebases."
  *TechDebt 2022*, ACM, pp. 11–20. arXiv: <https://arxiv.org/abs/2203.04374> ·
  DOI: <https://doi.org/10.1145/3524843.3528091> ·
  <https://codescene.com/blog/measuring-the-business-impact-of-low-code-quality>

### Fault concentration / "hotspots"

The idea that defects and effort concentrate in a small fraction of files is
genuinely supported by independent academic research (Fenton & Ohlsson 2000;
Ostrand/Weyuker/Bell) — though those studies tend to **reject a strict 80/20
law**. Tornhill popularized the *effort* framing in *Your Code as a Crime Scene*
(Pragmatic Bookshelf, 1st ed. 2015 / 2nd ed. 2024) and *Software Design X-Rays*
(2018). His specific "1–2% of the codebase → up to 70% of the work" numbers are
**illustrative and uncited** — cite the *concept* as grounded, the *percentages*
as illustrative.

### Martin Fowler — "Is High Quality Software Worth the Cost?" (2019)

A reasoned essay (not data). Key claims, verbatim: users "cannot tell the
difference between higher or lower internal quality"; "Cruft adds to the time it
take[s] for me to understand how to make a change"; and the payoff is fast —
**"Developers find poor quality code significantly slows them down within a few
weeks."** <https://martinfowler.com/articles/is-quality-worth-cost.html>

### Ward Cunningham — the original "technical debt" metaphor (1992)

Verbatim: *"Shipping first time code is like going into debt. … Every minute
spent on not-quite-right code counts as interest on that debt. Entire engineering
organizations can be brought to a stand-still under the debt load of an
unconsolidated implementation…"* This is the source of the "compound interest"
framing. Citation: W. Cunningham. "The WyCash Portfolio Management System."
*OOPSLA '92* Addendum. DOI: <https://doi.org/10.1145/157709.157715> ·
author's copy: <https://c2.com/doc/oopsla92.html>

### Industry surveys (directional, self-reported — caveat heavily)

- **Stripe, "The Developer Coefficient" (2018):** developers report ~**13.5
  hrs/week** on technical debt and ~3.8 hrs on "bad code" specifically; "bad
  code" ≈ $85B/yr global opportunity cost. >1,000 developers + >1,000
  executives. Vendor survey, self-reported estimates, macro extrapolation.
  <https://stripe.com/files/reports/the-developer-coefficient.pdf>
- **McKinsey, "Tech debt: Reclaiming tech equity" (2020):** CIOs estimated tech
  debt at **20–40% of the value of their technology estate** before depreciation
  (survey of only ~50 CIOs). All McKinsey figures here are **[unverified]**
  (primary blocked; secondary-sourced); the "Developer Velocity" and 2023
  "measure developer productivity" pieces were publicly criticized — cite with
  caveats.

### The one to *not* lean on — "100× to fix later"

The qualitative "later = costlier" idea is real (Boehm & Basili, "Software Defect
Reduction Top 10 List," *IEEE Computer* 34(1), 2001 — but the authors themselves
hedge it: "more like 5:1 than 100:1" for small systems). The **smooth
1×→100× exponential curve is folklore**: the famous "IBM Systems Sciences
Institute" chart has no traceable study, NIST's steep tables are marked "Example
Only," and the largest modern test found **no effect** (Menzies et al., "Are
Delayed Issues Harder to Resolve?", arXiv:1609.04886, 2016 — 171 projects). If
used at all, pair it with these caveats.

---

## 6. What this means for code-ranker's design

The evidence does not just justify the product — it *constrains* it, and
code-ranker's design already reflects that:

1. **Lead with cycles and coupling.** They are the best-supported signals and the
   product's genuine differentiator. ADP is treated as the top-priority smell.
   HK is the headline coupling metric — but be precise about its standing (§2):
   the *information-flow coupling idea* has broad support, while the exact
   `length × (fan_in × fan_out)²` formula is **not** independently validated and
   has documented flaws (it reads zero whenever fan-in or fan-out is zero). HK
   earns its place as a fast, transparent ranking heuristic — not as a proven law.
2. **Be candid about file-level complexity.** File-level `cyclomatic` ≈ `loc`
   (§4). The tool reports both, but the independent signal is in cognitive
   complexity, coupling, and cycles — not file cyclomatic "on its own."
3. **Calibrate, don't decree.** No universal threshold generalizes across
   projects (Nagappan et al. 2006). code-ranker calibrates `info`/`warning`
   thresholds per project rather than hardcoding magic numbers.
4. **Human-in-the-loop, by design.** Because these are correlational proxies, not
   proof, code-ranker emits a **ranked shortlist for human or AI-agent review** —
   not an automated verdict. A high HK can be a legitimate coordinator; a cycle
   can be deliberate. The tool points; a human decides.
5. **Don't over-claim.** Avoid the "100× to fix later" curve and unqualified
   vendor statistics. The honest, broad, decades-long correlational record is a
   stronger and more durable foundation than any single dramatic number.

---

## References (consolidated)

**Dependency cycles**
- Oyetoyan, Cruzes & Conradi (2013), *JSS* 86(12). <https://doi.org/10.1016/j.jss.2013.07.039>
- Oyetoyan, Falleri, Dietrich & Jezek (2015), *SANER*. <https://doi.org/10.1109/SANER.2015.7081834>
- Melton & Tempero (2007), *EMSE* 12(4). <https://doi.org/10.1007/s10664-006-9033-1>
- Martin (2017), *Clean Architecture*, Prentice Hall.

**Coupling / Henry-Kafura**
- Henry & Kafura (1981), *IEEE TSE* SE-7(5). <https://doi.org/10.1109/TSE.1981.231113>
- Kitchenham, Pickard & Linkman (1990), *Software Engineering Journal* 5(1). <https://doi.org/10.1049/sej.1990.0007>
- Shepperd (1990), *Software Engineering Journal* 5(1). <https://doi.org/10.1049/sej.1990.0002>
- Shepperd & Ince (1994), *JSS* 26(3). <https://doi.org/10.1016/0164-1212(94)90011-6>
- Card & Agresti (1988), *JSS* 8(3). <https://doi.org/10.1016/0164-1212(88)90021-0>
- Card & Glass (1990), *Measuring Software Design Quality*, Prentice Hall.
- Kafura & Reddy (1987, not independent), *IEEE TSE* SE-13(3). <https://doi.org/10.1109/TSE.1987.233164>
- Radjenović et al. (2013), *IST* 55(8). <https://doi.org/10.1016/j.infsof.2013.02.009>
- Subramanyam & Krishnan (2003), *IEEE TSE* 29(4). <https://doi.org/10.1109/TSE.2003.1191795>
- Basili, Briand & Melo (1996), *IEEE TSE* 22(10). <https://doi.org/10.1109/32.544352>
- Gyimóthy, Ferenc & Siket (2005), *IEEE TSE* 31(10). <https://doi.org/10.1109/TSE.2005.112>
- MacCormack, Rusnak & Baldwin (2006), *Management Science* 52(7). <https://doi.org/10.1287/mnsc.1060.0552>
- Sturtevant (2013), MIT PhD thesis. <https://dspace.mit.edu/handle/1721.1/79551>
- MacCormack & Sturtevant (2016), *JSS* 120. <https://doi.org/10.1016/j.jss.2016.06.007>
- Forsgren, Humble & Kim (2018), *Accelerate*, IT Revolution. DORA: <https://dora.dev/capabilities/loosely-coupled-teams/>
- Zimmermann & Nagappan (2008), *ICSE*. <https://doi.org/10.1145/1368088.1368161>

**Complexity / size & metric definitions**
- McCabe (1976), *IEEE TSE* SE-2(4). <https://doi.org/10.1109/TSE.1976.233837>
- Watson & McCabe, NIST SP 500-235 (1996). <https://www.nist.gov/publications/structured-testing-testing-methodology-using-cyclomatic-complexity-metric>
- Campbell (2018), *TechDebt*. <https://doi.org/10.1145/3194164.3194186> · <https://www.sonarsource.com/resources/cognitive-complexity/>
- Halstead (1977), *Elements of Software Science*, Elsevier. Critique: Hamer & Frewin (1982), *ICSE*. <https://dl.acm.org/doi/10.5555/800254.807762>
- Oman & Hagemeister (1994), *JSS* 24(3). <https://doi.org/10.1016/0164-1212(94)90067-1> · MI critique: <https://avandeursen.com/2014/08/29/think-twice-before-using-the-maintainability-index/>
- Khoshgoftaar et al. (1996), *IEEE Software* 13(1). <https://doi.org/10.1109/52.476287>
- Nagappan, Ball & Zeller (2006), *ICSE*. <https://doi.org/10.1145/1134285.1134349>

**Size-confound critique**
- Jay et al. (2009), *JSEA* 2(3). <https://doi.org/10.4236/jsea.2009.23020>
- El Emam et al. (2001), *IEEE TSE* 27(7). <https://doi.org/10.1109/32.935855>
- Fenton & Neil (1999), *IEEE TSE* 25(5). <https://doi.org/10.1109/32.815326>
- Gil & Lalouche (2017), *EMSE* 22(5). <https://doi.org/10.1007/s10664-017-9513-5>
- Landman et al. (2016), *JSEP* 28(7). <https://aserebre.win.tue.nl/Landman2015-ccsloc-jsep2015-preprint.pdf>

**Business cost**
- Tornhill & Borg (2022), *TechDebt*. <https://arxiv.org/abs/2203.04374> · <https://doi.org/10.1145/3524843.3528091>
- Tornhill, *Your Code as a Crime Scene* (2015/2024) & *Software Design X-Rays* (2018), Pragmatic Bookshelf.
- Fowler (2019), "Is High Quality Software Worth the Cost?" <https://martinfowler.com/articles/is-quality-worth-cost.html>
- Cunningham (1992), *OOPSLA* Addendum. <https://doi.org/10.1145/157709.157715> · <https://c2.com/doc/oopsla92.html>
- Stripe (2018), "The Developer Coefficient." <https://stripe.com/files/reports/the-developer-coefficient.pdf>
- Boehm & Basili (2001), *IEEE Computer* 34(1). · Menzies et al. (2016), arXiv:1609.04886. <https://arxiv.org/abs/1609.04886>

---

*Verification note: figures were checked against primary sources where possible.
Items marked **[unverified]** could not be confirmed from primary text and should
not be quoted as settled. Most evidence here is correlational, not causal — see
§4 and §6.*
