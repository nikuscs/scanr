//! Medium-confidence test-theater detectors.

use std::collections::{BTreeMap, BTreeSet};

use crate::slop::types::{
    AssertionApi, AssertionBoundary, AssertionFact, CallRole, Detector, DuplicatedTestGroup,
    ExpressionShape, FileFacts, MockFact, MockKind, ProductionExpressionFact, ProjectFacts,
    Resolution, SetupKind, SlopConfidence, SlopEvidence, SlopFinding, SlopKind, SourceSpan,
    SuiteFact, SuiteId, TestCaseFact, TestFramework, TestId, TestMode,
};

#[allow(dead_code)] // Exported for the shared registry owner.
pub fn detectors() -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(AssertionMonoculture::default()),
        Box::new(MockDominatedTest::default()),
        Box::new(DuplicatedTestBody::default()),
        Box::new(ImplementationMirroringTest::default()),
    ]
}

/// Exposes the exact normalized literal-only groups already consumed by the
/// duplicated-test detector, so diff analysis does not reparse or regex test bodies.
pub fn duplicated_test_groups(project: &ProjectFacts) -> Vec<DuplicatedTestGroup> {
    let mut result = Vec::new();
    for (_, tests) in eligible_suites(project) {
        if excluded_tests_dominate(project, &tests) {
            continue;
        }
        let mut groups: BTreeMap<String, Vec<&TestCaseFact>> = BTreeMap::new();
        for test in tests.iter().copied().filter(|test| {
            test_eligible(project, test)
                && test.body_shape.statement_count >= 2
                && test.body_shape.node_count >= 6
        }) {
            groups.entry(test.body_shape.canonical.clone()).or_default().push(test);
        }
        for mut group in groups.into_values() {
            group.sort_by(|left, right| left.id.cmp(&right.id));
            let literal_lengths = group
                .iter()
                .map(|test| test.body_shape.literal_vector.len())
                .collect::<BTreeSet<_>>();
            let literal_vectors = group
                .iter()
                .map(|test| test.body_shape.literal_vector.clone())
                .collect::<BTreeSet<_>>();
            if group.len() >= 4
                && literal_lengths.len() == 1
                && literal_vectors.len() >= 2
                && !group.iter().all(|test| concise_assertion_fixture(project, test))
            {
                result.push(DuplicatedTestGroup {
                    tests: group.into_iter().map(|test| test.id.clone()).collect(),
                    literals_only_difference: true,
                });
            }
        }
    }
    result.sort_by(|left, right| left.tests.cmp(&right.tests));
    result
}

struct AssertionMonoculture {
    min_tests: usize,
    dominant_percent: u8,
}

impl Default for AssertionMonoculture {
    fn default() -> Self {
        Self { min_tests: 6, dominant_percent: 80 }
    }
}

impl Detector for AssertionMonoculture {
    fn kind(&self) -> SlopKind {
        SlopKind::AssertionMonoculture
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for (suite_id, tests) in eligible_suites(project) {
            if excluded_tests_dominate(project, &tests) {
                continue;
            }
            let mut selected = Vec::new();
            let mut total_assertions = 0usize;
            let mut matchers = BTreeSet::new();
            let mut boundaries = BTreeSet::new();
            let mut counter_signal = false;
            let mut ambiguous_assertion = false;
            for test in tests.iter().copied().filter(|test| test_eligible(project, test)) {
                let assertions = assertions_for_test(project, &test.id);
                if assertions.is_empty() {
                    continue;
                }
                if assertions.iter().any(|assertion| !assertion_recognized(assertion)) {
                    ambiguous_assertion = true;
                    continue;
                }
                total_assertions += assertions.len();
                let shapes = assertions
                    .iter()
                    .map(|assertion| AssertionShape::from(assertion))
                    .collect::<BTreeSet<_>>();
                for assertion in assertions {
                    matchers.insert(assertion.matcher.clone());
                    if assertion.boundary != AssertionBoundary::None {
                        boundaries.insert(assertion.boundary);
                    }
                    counter_signal |= assertion.boundary != AssertionBoundary::None
                        || assertion.negated
                        || assertion.async_modifier.as_deref() == Some("rejects");
                }
                selected.push((test, shapes));
            }
            if ambiguous_assertion
                || selected.len() < self.min_tests
                || total_assertions < self.min_tests
                || counter_signal
                || matchers.len() >= 2
                || boundaries.len() >= 2
            {
                continue;
            }

            let mut counts: BTreeMap<AssertionShape, Vec<&TestCaseFact>> = BTreeMap::new();
            for (test, shapes) in &selected {
                for shape in shapes {
                    counts.entry(shape.clone()).or_default().push(test);
                }
            }
            let Some((shape, mut dominant_tests)) = counts.into_iter().max_by(|left, right| {
                left.1.len().cmp(&right.1.len()).then_with(|| right.0.cmp(&left.0))
            }) else {
                continue;
            };
            dominant_tests
                .sort_by(|left, right| left.registration_span.cmp(&right.registration_span));
            let dominant = dominant_tests.len();
            let total = selected.len();
            if dominant < self.min_tests
                || dominant * 100 < usize::from(self.dominant_percent) * total
            {
                continue;
            }

            let primary = suite_span(project, &suite_id, &dominant_tests);
            let suite_name = qualified_suite_name(project, &suite_id);
            let mut evidence = vec![summary_evidence(
                &primary,
                "assertion-monoculture-summary",
                format!(
                    "suite={suite_name};eligibleTests={total};dominantTests={dominant};percentage={};shape={}",
                    dominant * 100 / total,
                    shape.canonical()
                ),
            )];
            evidence.extend(dominant_tests.iter().take(6).map(|test| {
                test_evidence(project, "dominant-assertion", test, &shape.canonical())
            }));
            evidence.sort_by(evidence_order);
            findings.push(SlopFinding {
                kind: self.kind(),
                confidence: self.confidence(),
                span: primary,
                evidence,
                explanation: format!(
                    "{dominant} of {total} eligible tests in {suite_name} use the same recognized assertion shape; no recognized boundary/error assertion was found"
                ),
                action: "add behavior-specific boundary and failure assertions, or consolidate intentional cases into a table".to_string(),
            });
        }
        sort_findings(&mut findings);
        findings
    }
}

struct MockDominatedTest {
    min_tests: usize,
    dominated_percent: u8,
}

impl Default for MockDominatedTest {
    fn default() -> Self {
        Self { min_tests: 4, dominated_percent: 75 }
    }
}

impl Detector for MockDominatedTest {
    fn kind(&self) -> SlopKind {
        SlopKind::MockDominatedTest
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    #[allow(clippy::too_many_lines)] // The count, suppression, and evidence gates stay together.
    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for (suite_id, tests) in eligible_suites(project) {
            if excluded_tests_dominate(project, &tests) {
                continue;
            }
            let mut eligible = Vec::new();
            for test in tests.iter().copied().filter(|test| test_eligible(project, test)) {
                let assertions = assertions_for_test(project, &test.id);
                if assertions.is_empty()
                    || assertions.iter().any(|fact| !assertion_recognized(fact))
                {
                    continue;
                }
                let sut_count = calls_with_role(project, test, CallRole::Sut).len();
                if sut_count == 0 {
                    continue;
                }
                let direct_mocks = direct_mocks(project, &test.id);
                let hook_mocks = before_each_mocks(project, test);
                if direct_mocks.iter().chain(&hook_mocks).any(|mock| !mock.resolution_complete) {
                    continue;
                }
                let mock_count = direct_mocks.len() + hook_mocks.len();
                eligible.push(TestCounts {
                    test,
                    mock_count,
                    sut_count,
                    assertion_count: assertions.len(),
                    direct_mock_count: direct_mocks.len(),
                    hook_ids: hook_mocks.iter().filter_map(|mock| mock.setup.clone()).collect(),
                });
            }
            if eligible.len() < self.min_tests || centralized_harness_only(&eligible) {
                continue;
            }
            let mut dominated = eligible
                .iter()
                .filter(|counts| {
                    counts.mock_count > counts.sut_count + counts.assertion_count
                        && counts.mock_count >= 3
                })
                .collect::<Vec<_>>();
            let aggregate_mocks = eligible.iter().map(|counts| counts.mock_count).sum::<usize>();
            let aggregate_denominator = eligible
                .iter()
                .map(|counts| counts.sut_count + counts.assertion_count)
                .sum::<usize>();
            if dominated.len() < 3
                || dominated.len() * 100 < usize::from(self.dominated_percent) * eligible.len()
                || aggregate_mocks <= aggregate_denominator
            {
                continue;
            }
            dominated.sort_by(|left, right| {
                let left_margin = left.mock_count - left.sut_count - left.assertion_count;
                let right_margin = right.mock_count - right.sut_count - right.assertion_count;
                right_margin
                    .cmp(&left_margin)
                    .then_with(|| left.test.registration_span.cmp(&right.test.registration_span))
            });
            let suite_name = qualified_suite_name(project, &suite_id);
            let primary = suite_span(
                project,
                &suite_id,
                &dominated.iter().map(|counts| counts.test).collect::<Vec<_>>(),
            );
            let mut evidence = vec![summary_evidence(
                &primary,
                "mock-dominance-summary",
                format!(
                    "suite={suite_name};eligibleTests={};dominatedTests={};mocks={aggregate_mocks};sutCallsAndAssertions={aggregate_denominator}",
                    eligible.len(),
                    dominated.len()
                ),
            )];
            evidence.extend(dominated.iter().take(4).map(|counts| {
                test_evidence(
                    project,
                    "mock-dominated-test",
                    counts.test,
                    &format!(
                        "mocks={};sutCalls={};assertions={}",
                        counts.mock_count, counts.sut_count, counts.assertion_count
                    ),
                )
            }));
            evidence.sort_by(evidence_order);
            findings.push(SlopFinding {
                kind: self.kind(),
                confidence: self.confidence(),
                span: primary,
                evidence,
                explanation: format!(
                    "{} of {} eligible tests in {suite_name} contain more recognized mock operations than recognized SUT calls and assertions",
                    dominated.len(),
                    eligible.len()
                ),
                action: "exercise the real boundary where practical and keep mocks at external seams"
                    .to_string(),
            });
        }
        sort_findings(&mut findings);
        findings
    }
}

#[allow(clippy::struct_field_names)] // Names mirror the reviewed Phase 6 thresholds.
struct DuplicatedTestBody {
    min_group: usize,
    min_statements: u16,
    min_nodes: u16,
}

impl Default for DuplicatedTestBody {
    fn default() -> Self {
        Self { min_group: 4, min_statements: 2, min_nodes: 6 }
    }
}

impl Detector for DuplicatedTestBody {
    fn kind(&self) -> SlopKind {
        SlopKind::DuplicatedTestBody
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for (suite_id, tests) in eligible_suites(project) {
            if excluded_tests_dominate(project, &tests) {
                continue;
            }
            let mut groups: BTreeMap<String, Vec<&TestCaseFact>> = BTreeMap::new();
            for test in tests.iter().copied().filter(|test| {
                test_eligible(project, test)
                    && test.body_shape.statement_count >= self.min_statements
                    && test.body_shape.node_count >= self.min_nodes
            }) {
                groups.entry(test.body_shape.canonical.clone()).or_default().push(test);
            }
            for (canonical, mut group) in groups {
                if group.len() < self.min_group {
                    continue;
                }
                group.sort_by(|left, right| left.registration_span.cmp(&right.registration_span));
                let literal_lengths = group
                    .iter()
                    .map(|test| test.body_shape.literal_vector.len())
                    .collect::<BTreeSet<_>>();
                let literal_vectors = group
                    .iter()
                    .map(|test| test.body_shape.literal_vector.clone())
                    .collect::<BTreeSet<_>>();
                if literal_lengths.len() != 1
                    || literal_vectors.len() < 2
                    || group.iter().all(|test| concise_assertion_fixture(project, test))
                {
                    continue;
                }
                let suite_name = qualified_suite_name(project, &suite_id);
                let primary = suite_span(project, &suite_id, &group);
                let mut evidence = group
                    .iter()
                    .map(|test| {
                        test_evidence(
                            project,
                            "duplicated-test-body",
                            test,
                            &format!("literals={:?}", test.body_shape.literal_vector),
                        )
                    })
                    .collect::<Vec<_>>();
                evidence.push(summary_evidence(
                    &primary,
                    "duplicated-test-body-summary",
                    format!(
                        "suite={suite_name};groupSize={};statements={};nodes={};literalVectors={};canonical={canonical}",
                        group.len(),
                        group[0].body_shape.statement_count,
                        group[0].body_shape.node_count,
                        literal_vectors.len()
                    ),
                ));
                evidence.sort_by(evidence_order);
                findings.push(SlopFinding {
                    kind: self.kind(),
                    confidence: self.confidence(),
                    span: primary,
                    evidence,
                    explanation: format!(
                        "{} eligible tests in {suite_name} have the same exact AST-backed body shape while their literal vectors vary",
                        group.len()
                    ),
                    action: "use an explicit parameter table if the cases intentionally differ only by data"
                        .to_string(),
                });
            }
        }
        sort_findings(&mut findings);
        findings
    }
}

struct ImplementationMirroringTest {
    min_tests: usize,
}

impl Default for ImplementationMirroringTest {
    fn default() -> Self {
        Self { min_tests: 2 }
    }
}

impl Detector for ImplementationMirroringTest {
    fn kind(&self) -> SlopKind {
        SlopKind::ImplementationMirroringTest
    }

    fn confidence(&self) -> SlopConfidence {
        SlopConfidence::Medium
    }

    fn detect(&self, project: &ProjectFacts) -> Vec<SlopFinding> {
        let mut findings = Vec::new();
        for (suite_id, tests) in eligible_suites(project) {
            if excluded_tests_dominate(project, &tests) {
                continue;
            }
            let mut matches: BTreeMap<crate::slop::types::SymbolKey, Vec<MirrorMatch<'_>>> =
                BTreeMap::new();
            for test in tests.iter().copied().filter(|test| test_eligible(project, test)) {
                for assertion in assertions_for_test(project, &test.id) {
                    let Some(mirror) = mirror_match(project, test, assertion) else { continue };
                    matches.entry(mirror.production.owner.clone()).or_default().push(mirror);
                }
            }
            for (symbol, mut group) in matches {
                group.sort_by(|left, right| left.assertion.span.cmp(&right.assertion.span));
                group.dedup_by(|left, right| left.test.id == right.test.id);
                if group.len() < self.min_tests {
                    continue;
                }
                let suite_name = qualified_suite_name(project, &suite_id);
                let production = group[0].production;
                let primary = group[0].test.registration_span.clone();
                let mut evidence = vec![SlopEvidence {
                    code: "production-return-expression".to_string(),
                    label: symbol.name.clone(),
                    span: production.expression_span.clone(),
                    detail: format!(
                        "complexity={};normalized={}",
                        production.returned.complexity, group[0].normalized
                    ),
                }];
                evidence.extend(group.iter().map(|item| SlopEvidence {
                    code: "mirroring-assertion".to_string(),
                    label: test_name(project, item.test),
                    span: item.assertion.span.clone(),
                    detail: format!(
                        "matcher={};normalized={}",
                        item.assertion.matcher, item.normalized
                    ),
                }));
                findings.push(SlopFinding {
                    kind: self.kind(),
                    confidence: self.confidence(),
                    span: primary,
                    evidence,
                    explanation: format!(
                        "{} eligible tests in {suite_name} structurally repeat one statically resolved production return expression for {}",
                        group.len(),
                        symbol.name
                    ),
                    action: "assert externally observable examples or invariants without recomputing the implementation expression".to_string(),
                });
            }
        }
        sort_findings(&mut findings);
        findings
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AssertionShape {
    api: AssertionApi,
    matcher: String,
    negated: bool,
    async_modifier: Option<String>,
    actual_root: String,
}

impl AssertionShape {
    fn from(assertion: &AssertionFact) -> Self {
        Self {
            api: assertion.api,
            matcher: assertion.matcher.clone(),
            negated: assertion.negated,
            async_modifier: assertion.async_modifier.clone(),
            actual_root: assertion
                .actual
                .as_ref()
                .map_or_else(|| "Other".to_string(), |shape| expression_root(&shape.canonical)),
        }
    }

    fn canonical(&self) -> String {
        format!(
            "api={:?};matcher={};negated={};async={};actualRoot={}",
            self.api,
            self.matcher,
            self.negated,
            self.async_modifier.as_deref().unwrap_or("none"),
            self.actual_root
        )
    }
}

struct TestCounts<'a> {
    test: &'a TestCaseFact,
    mock_count: usize,
    sut_count: usize,
    assertion_count: usize,
    direct_mock_count: usize,
    hook_ids: BTreeSet<crate::slop::types::SetupId>,
}

struct MirrorMatch<'a> {
    test: &'a TestCaseFact,
    assertion: &'a AssertionFact,
    production: &'a ProductionExpressionFact,
    normalized: String,
}

fn eligible_suites(project: &ProjectFacts) -> BTreeMap<SuiteId, Vec<&TestCaseFact>> {
    let mut suites: BTreeMap<SuiteId, Vec<&TestCaseFact>> = BTreeMap::new();
    for test in project.tests.values() {
        let Some(file) = project.files.get(&test.id.path) else { continue };
        if file_eligible(file) {
            suites.entry(test.suite.clone()).or_default().push(test);
        }
    }
    for tests in suites.values_mut() {
        tests.sort_by(|left, right| left.registration_span.cmp(&right.registration_span));
    }
    suites
}

const fn file_eligible(file: &FileFacts) -> bool {
    file.is_test && file.analysis_complete && !file.is_generated
}

fn test_eligible(project: &ProjectFacts, test: &TestCaseFact) -> bool {
    let suite_complete =
        suite_ancestors(project, &test.suite).iter().all(|suite| suite.resolution_complete);
    matches!(test.mode, TestMode::Run | TestMode::Only)
        && test.framework != TestFramework::Unknown
        && !test.has_snapshot
        && !test.has_unknown_dynamic_call
        && suite_complete
}

fn excluded_tests_dominate(project: &ProjectFacts, tests: &[&TestCaseFact]) -> bool {
    let executable = tests
        .iter()
        .filter(|test| {
            !matches!(test.mode, TestMode::Skip | TestMode::Todo | TestMode::DisabledAlias)
        })
        .count();
    let excluded = tests
        .iter()
        .filter(|test| {
            !matches!(test.mode, TestMode::Skip | TestMode::Todo | TestMode::DisabledAlias)
                && !test_eligible(project, test)
        })
        .count();
    executable > 0 && excluded * 2 >= executable
}

fn assertions_for_test<'a>(project: &'a ProjectFacts, id: &TestId) -> Vec<&'a AssertionFact> {
    project.assertions.values().filter(|assertion| assertion.test.as_ref() == Some(id)).collect()
}

fn assertion_recognized(assertion: &AssertionFact) -> bool {
    assertion.api != AssertionApi::UnknownAssert
        && matches!(assertion.api_resolution, Resolution::Resolved(_))
        && !assertion.is_snapshot
}

fn direct_mocks<'a>(project: &'a ProjectFacts, id: &TestId) -> Vec<&'a MockFact> {
    project
        .mocks
        .values()
        .filter(|mock| {
            mock.test.as_ref() == Some(id) && mock.setup.is_none() && mock.kind != MockKind::Restore
        })
        .collect()
}

fn before_each_mocks<'a>(project: &'a ProjectFacts, test: &TestCaseFact) -> Vec<&'a MockFact> {
    let suites = suite_ancestors(project, &test.suite)
        .into_iter()
        .map(|suite| suite.id.clone())
        .collect::<BTreeSet<_>>();
    project
        .mocks
        .values()
        .filter(|mock| {
            mock.kind != MockKind::Restore
                && mock.test.is_none()
                && mock.setup.as_ref().is_some_and(|setup_id| {
                    project.setups.get(setup_id).is_some_and(|setup| {
                        setup.kind == SetupKind::BeforeEach
                            && setup.resolution_complete
                            && suites.contains(&setup.suite)
                    })
                })
        })
        .collect()
}

fn calls_with_role<'a>(
    project: &'a ProjectFacts,
    test: &TestCaseFact,
    role: CallRole,
) -> Vec<&'a crate::slop::types::CallFact> {
    let Some(file) = project.files.get(&test.id.path) else { return Vec::new() };
    let mut calls = file
        .calls
        .iter()
        .filter(|call| {
            call.span.start_byte >= test.body_span.start_byte
                && call.span.end_byte <= test.body_span.end_byte
                && project.call_roles.get(&call.key) == Some(&role)
        })
        .collect::<Vec<_>>();
    calls.sort_by(|left, right| left.span.cmp(&right.span));
    calls.dedup_by(|left, right| left.span == right.span);
    calls
}

fn centralized_harness_only(counts: &[TestCounts<'_>]) -> bool {
    let hook_ids =
        counts.iter().flat_map(|counts| counts.hook_ids.iter().cloned()).collect::<BTreeSet<_>>();
    hook_ids.len() == 1
        && counts.iter().all(|counts| counts.direct_mock_count <= 1)
        && counts.iter().all(|counts| !counts.hook_ids.is_empty())
}

fn concise_assertion_fixture(project: &ProjectFacts, test: &TestCaseFact) -> bool {
    if test.body_shape.statement_count > 2 || assertions_for_test(project, &test.id).len() != 1 {
        return false;
    }
    let sut = calls_with_role(project, test, CallRole::Sut).len();
    let fixture = calls_with_role(project, test, CallRole::Fixture).len();
    sut == 0 && fixture <= 1
}

fn mirror_match<'a>(
    project: &'a ProjectFacts,
    test: &'a TestCaseFact,
    assertion: &'a AssertionFact,
) -> Option<MirrorMatch<'a>> {
    if !assertion_recognized(assertion)
        || assertion.boundary != AssertionBoundary::None
        || assertion.negated
        || assertion.async_modifier.as_deref() == Some("rejects")
        || suppressed_mirror_matcher(&assertion.matcher)
    {
        return None;
    }
    let expected = assertion.expected.as_ref()?;
    if !expected.resolution_complete || trivial_expected(expected) {
        return None;
    }
    let Resolution::Resolved(symbol) = &assertion.invokes else { return None };
    let production = project.production_expressions.get(symbol)?;
    if !production.eligible
        || production.ambiguity.is_some()
        || production.returned.complexity < 3
        || !production.returned.resolution_complete
        || contains_literal_shape(&production.returned.canonical)
        || contains_literal_shape(&expected.canonical)
    {
        return None;
    }
    let call = assertion.invoked_call.as_ref()?;
    let arguments = project.call_arguments.get(call)?;
    if arguments.has_spread
        || arguments.arguments.len() != production.parameter_names.len()
        || arguments.arguments.iter().any(|argument| {
            !argument.resolution_complete
                || matches!(
                    expression_root(&argument.canonical).as_str(),
                    "ArrowFunctionExpression" | "FunctionExpression"
                )
        })
    {
        return None;
    }
    let production_normalized = normalize_production(production);
    let expected_normalized = normalize_expected(expected, &arguments.arguments);
    if production_normalized != expected_normalized
        || !has_non_placeholder_structure(&production_normalized)
    {
        return None;
    }
    Some(MirrorMatch { test, assertion, production, normalized: production_normalized })
}

fn normalize_production(production: &ProductionExpressionFact) -> String {
    let mut canonical = production.returned.canonical.clone();
    for (index, parameter) in production.parameter_names.iter().enumerate() {
        canonical =
            canonical.replace(&format!("(IdentifierReference:{parameter})"), &format!("$p{index}"));
    }
    canonical
}

fn normalize_expected(expected: &ExpressionShape, arguments: &[ExpressionShape]) -> String {
    let mut canonical = expected.canonical.clone();
    let mut replacements = arguments
        .iter()
        .enumerate()
        .map(|(index, argument)| (argument.canonical.as_str(), format!("$p{index}")))
        .collect::<Vec<_>>();
    replacements
        .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then_with(|| left.1.cmp(&right.1)));
    for (argument, placeholder) in replacements {
        canonical = canonical.replace(argument, &placeholder);
    }
    canonical
}

fn contains_literal_shape(canonical: &str) -> bool {
    [
        "StringLiteral",
        "NumericLiteral",
        "BooleanLiteral",
        "NullLiteral",
        "BigIntLiteral",
        "RegExpLiteral",
        "TemplateElement",
    ]
    .iter()
    .any(|kind| canonical.contains(kind))
}

fn has_non_placeholder_structure(canonical: &str) -> bool {
    canonical.contains("BinaryExpression")
        || canonical.contains("LogicalExpression")
        || canonical.contains("ConditionalExpression")
        || canonical.contains("CallExpression")
        || canonical.contains("MemberExpression")
}

fn suppressed_mirror_matcher(matcher: &str) -> bool {
    let lower = matcher.to_ascii_lowercase();
    lower.contains("snapshot")
        || lower.contains("throw")
        || lower.contains("reject")
        || lower.contains("closeto")
        || lower.contains("schema")
        || matches!(lower.as_str(), "tomatch" | "match" | "satisfies" | "topass")
}

fn trivial_expected(expected: &ExpressionShape) -> bool {
    matches!(
        expression_root(&expected.canonical).as_str(),
        "StringLiteral"
            | "NumericLiteral"
            | "BooleanLiteral"
            | "NullLiteral"
            | "BigIntLiteral"
            | "RegExpLiteral"
            | "IdentifierReference"
            | "StaticMemberExpression"
            | "ComputedMemberExpression"
            | "ArrayExpression"
            | "ObjectExpression"
    )
}

fn expression_root(canonical: &str) -> String {
    canonical
        .strip_prefix('(')
        .unwrap_or(canonical)
        .split([':', '(', ')'])
        .next()
        .unwrap_or("Other")
        .to_string()
}

fn suite_ancestors<'a>(project: &'a ProjectFacts, id: &SuiteId) -> Vec<&'a SuiteFact> {
    let mut result = Vec::new();
    let mut current = Some(id.clone());
    let mut seen = BTreeSet::new();
    while let Some(id) = current.take() {
        if !seen.insert(id.clone()) {
            break;
        }
        let Some(suite) = project.suites.get(&id) else { break };
        current.clone_from(&suite.parent);
        result.push(suite);
    }
    result
}

fn qualified_suite_name(project: &ProjectFacts, id: &SuiteId) -> String {
    if id.registration_start == 0 {
        return "<file>".to_string();
    }
    let mut names = suite_ancestors(project, id)
        .into_iter()
        .rev()
        .filter(|suite| suite.id.registration_start != 0)
        .map(|suite| {
            suite.name.clone().unwrap_or_else(|| format!("<suite@{}>", suite.span.start_line))
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        names.push("<file>".to_string());
    }
    names.join(" > ")
}

fn suite_span(project: &ProjectFacts, id: &SuiteId, tests: &[&TestCaseFact]) -> SourceSpan {
    project
        .suites
        .get(id)
        .filter(|_| id.registration_start != 0)
        .map(|suite| suite.span.clone())
        .or_else(|| tests.first().map(|test| test.registration_span.clone()))
        .unwrap_or_else(|| empty_span(&id.path))
}

fn test_name(project: &ProjectFacts, test: &TestCaseFact) -> String {
    project
        .files
        .get(&test.id.path)
        .and_then(|file| {
            file.tests
                .iter()
                .find(|legacy| legacy.call_span.start_byte == test.registration_span.start_byte)
        })
        .and_then(|legacy| legacy.name.clone())
        .unwrap_or_else(|| format!("<test@{}>", test.registration_span.start_line))
}

fn test_evidence(
    project: &ProjectFacts,
    code: &str,
    test: &TestCaseFact,
    detail: &str,
) -> SlopEvidence {
    SlopEvidence {
        code: code.to_string(),
        label: test_name(project, test),
        span: test.registration_span.clone(),
        detail: detail.to_string(),
    }
}

fn summary_evidence(span: &SourceSpan, code: &str, detail: String) -> SlopEvidence {
    SlopEvidence {
        code: code.to_string(),
        label: "recognized evidence summary".to_string(),
        span: span.clone(),
        detail,
    }
}

fn empty_span(path: &str) -> SourceSpan {
    SourceSpan {
        path: path.to_string(),
        start_byte: 0,
        end_byte: 0,
        start_line: 1,
        start_column: 1,
        end_line: 1,
        end_column: 1,
    }
}

fn evidence_order(left: &SlopEvidence, right: &SlopEvidence) -> std::cmp::Ordering {
    left.span
        .cmp(&right.span)
        .then_with(|| left.code.cmp(&right.code))
        .then_with(|| left.detail.cmp(&right.detail))
}

fn sort_findings(findings: &mut [SlopFinding]) {
    findings.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.span.cmp(&right.span))
            .then_with(|| left.explanation.cmp(&right.explanation))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slop::types::{
        CallArgumentFact, CallFact, CallSiteKey, ScopeKey, SetupFact, SetupId, SymbolKey,
        TestBodyShape, TestFact,
    };

    fn span(path: &str, start: u32) -> SourceSpan {
        SourceSpan {
            path: path.to_string(),
            start_byte: start,
            end_byte: start + 5,
            start_line: start / 100 + 1,
            start_column: 1,
            end_line: start / 100 + 1,
            end_column: 6,
        }
    }

    fn shape(canonical: &str, complexity: u16) -> ExpressionShape {
        ExpressionShape {
            canonical: canonical.to_string(),
            complexity,
            call_chain: Vec::new(),
            referenced_symbols: Vec::new(),
            resolution_complete: true,
        }
    }

    fn root_suite(path: &str) -> SuiteFact {
        SuiteFact {
            id: SuiteId { path: path.to_string(), registration_start: 0 },
            parent: None,
            name: None,
            span: span(path, 0),
            callback_span: None,
            resolution_complete: true,
        }
    }

    fn test_case(path: &str, start: u32) -> TestCaseFact {
        TestCaseFact {
            id: TestId {
                path: path.to_string(),
                callback_start: start + 1,
                registration_start: start,
            },
            suite: SuiteId { path: path.to_string(), registration_start: 0 },
            framework: TestFramework::JestLike,
            registration_span: span(path, start),
            callback_span: SourceSpan { end_byte: start + 90, ..span(path, start + 1) },
            body_span: SourceSpan { end_byte: start + 80, ..span(path, start + 2) },
            mode: TestMode::Run,
            body_shape: TestBodyShape {
                canonical: "body".to_string(),
                literal_vector: vec![format!("{start}")],
                statement_count: 3,
                node_count: 10,
            },
            has_snapshot: false,
            has_unknown_dynamic_call: false,
        }
    }

    fn assertion(test: &TestCaseFact, start: u32, matcher: &str) -> AssertionFact {
        AssertionFact {
            span: span(&test.id.path, start),
            test: Some(test.id.clone()),
            api: AssertionApi::Expect,
            api_resolution: Resolution::Resolved("vitest".to_string()),
            matcher: matcher.to_string(),
            negated: false,
            async_modifier: None,
            boundary: AssertionBoundary::None,
            actual: Some(shape("(CallExpression(IdentifierReference:sut))", 1)),
            expected: Some(shape("(BooleanLiteral:bool)", 0)),
            invoked_call: None,
            invokes: Resolution::Unknown { reason: "not needed".to_string() },
            is_snapshot: false,
        }
    }

    fn base_project(test_count: usize) -> ProjectFacts {
        let path = "suite.test.ts";
        let suite = root_suite(path);
        let mut file = FileFacts {
            path: path.to_string(),
            analysis_complete: true,
            is_test: true,
            suites: vec![suite.clone()],
            ..FileFacts::default()
        };
        let mut tests = BTreeMap::new();
        let mut assertions = BTreeMap::new();
        for index in 0..test_count {
            let test = test_case(path, (index as u32) * 100);
            let fact = assertion(&test, test.id.registration_start + 20, "toBe");
            file.test_cases.push(test.clone());
            file.assertions.push(fact.clone());
            file.tests.push(TestFact {
                call_span: test.registration_span.clone(),
                name: Some(format!("case-{index}")),
                mode: TestMode::Run,
                callback_span: Some(test.callback_span.clone()),
                callback_resolution_complete: true,
                assertion_spans: vec![fact.span.clone()],
                mock_spans: Vec::new(),
                body_canonical: Some(test.body_shape.canonical.clone()),
                literal_vector: test.body_shape.literal_vector.clone(),
            });
            tests.insert(test.id.clone(), test);
            assertions.insert(fact.span.clone(), fact);
        }
        ProjectFacts {
            files: BTreeMap::from([(path.to_string(), file)]),
            suites: BTreeMap::from([(suite.id.clone(), suite)]),
            tests,
            assertions,
            ..ProjectFacts::default()
        }
    }

    #[test]
    fn registry_is_stable_and_medium_confidence() {
        let registry = detectors();
        assert_eq!(
            registry.iter().map(|detector| detector.kind()).collect::<Vec<_>>(),
            vec![
                SlopKind::AssertionMonoculture,
                SlopKind::MockDominatedTest,
                SlopKind::DuplicatedTestBody,
                SlopKind::ImplementationMirroringTest,
            ]
        );
        assert!(registry.iter().all(|detector| detector.confidence() == SlopConfidence::Medium));
    }

    #[test]
    fn assertion_monoculture_uses_rich_shapes_and_boundary_suppression() {
        let mut project = base_project(6);
        let findings = AssertionMonoculture::default().detect(&project);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].explanation.contains("recognized boundary/error"));

        let first = project.assertions.values_mut().next().unwrap();
        first.boundary = AssertionBoundary::BooleanEdge;
        assert!(AssertionMonoculture::default().detect(&project).is_empty());
    }

    #[test]
    fn parameterized_half_suppresses_suite() {
        let mut project = base_project(12);
        for test in project.tests.values_mut().skip(6) {
            test.mode = TestMode::Parameterized;
        }
        assert!(AssertionMonoculture::default().detect(&project).is_empty());
    }

    #[test]
    fn mock_dominance_counts_direct_mocks_and_sut_roles() {
        let mut project = base_project(4);
        let path = "suite.test.ts";
        let tests = project.tests.values().cloned().collect::<Vec<_>>();
        for test in tests {
            let sut_start = test.id.registration_start + 30;
            let sut_key = CallSiteKey { path: path.to_string(), start_byte: sut_start };
            project.files.get_mut(path).unwrap().calls.push(CallFact {
                key: sut_key.clone(),
                span: span(path, sut_start),
                scope: ScopeKey::Test {
                    path: path.to_string(),
                    call_start: test.id.registration_start,
                },
                callee_path: vec!["sut".to_string()],
            });
            project.call_roles.insert(sut_key, CallRole::Sut);
            for offset in [40, 45, 50] {
                let mock = MockFact {
                    span: span(path, test.id.registration_start + offset),
                    test: Some(test.id.clone()),
                    suite: test.suite.clone(),
                    setup: None,
                    kind: MockKind::Factory,
                    callee: "vi.fn".to_string(),
                    resolution_complete: true,
                };
                project.mocks.insert(mock.span.clone(), mock);
            }
        }
        let findings = MockDominatedTest::default().detect(&project);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].explanation.contains("recognized SUT calls"));
    }

    #[test]
    fn one_shared_before_each_harness_is_suppressed() {
        let mut project = base_project(4);
        let path = "suite.test.ts";
        let setup_id = SetupId { path: path.to_string(), registration_start: 900 };
        project.setups.insert(
            setup_id.clone(),
            SetupFact {
                id: setup_id.clone(),
                suite: SuiteId { path: path.to_string(), registration_start: 0 },
                kind: SetupKind::BeforeEach,
                registration_span: span(path, 900),
                callback_span: Some(span(path, 901)),
                resolution_complete: true,
            },
        );
        for offset in [910, 920, 930] {
            let mock = MockFact {
                span: span(path, offset),
                test: None,
                suite: SuiteId { path: path.to_string(), registration_start: 0 },
                setup: Some(setup_id.clone()),
                kind: MockKind::Factory,
                callee: "vi.fn".to_string(),
                resolution_complete: true,
            };
            project.mocks.insert(mock.span.clone(), mock);
        }
        for test in project.tests.values() {
            let start = test.id.registration_start + 30;
            let key = CallSiteKey { path: path.to_string(), start_byte: start };
            project.files.get_mut(path).unwrap().calls.push(CallFact {
                key: key.clone(),
                span: span(path, start),
                scope: ScopeKey::Module(path.to_string()),
                callee_path: vec!["sut".to_string()],
            });
            project.call_roles.insert(key, CallRole::Sut);
        }
        assert!(MockDominatedTest::default().detect(&project).is_empty());
    }

    #[test]
    fn duplicated_body_requires_exact_shape_and_varying_literals() {
        let mut project = base_project(4);
        for (index, test) in project.tests.values_mut().enumerate() {
            test.body_shape.canonical = "(FunctionBody(ExpressionStatement(CallExpression))(ExpressionStatement(CallExpression)))".to_string();
            test.body_shape.literal_vector =
                vec![format!("endpoint-{index}"), format!("{}", 200 + index)];
        }
        let findings = DuplicatedTestBody::default().detect(&project);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].explanation.contains("exact AST-backed body shape"));

        for test in project.tests.values_mut() {
            test.body_shape.literal_vector = vec!["same".to_string(), "200".to_string()];
        }
        assert!(DuplicatedTestBody::default().detect(&project).is_empty());
    }

    #[test]
    fn implementation_mirroring_groups_two_tests_by_resolved_production_symbol() {
        let mut project = base_project(2);
        let owner = SymbolKey {
            path: "src/normalize.ts".to_string(),
            declaration_start: 1,
            name: "normalize".to_string(),
        };
        let production_canonical = "(CallExpression(StaticMemberExpression(CallExpression(IdentifierReference:normalize)(IdentifierReference:input))(IdentifierName:trim)))";
        project.production_expressions.insert(
            owner.clone(),
            ProductionExpressionFact {
                owner: owner.clone(),
                owner_span: span("src/normalize.ts", 1),
                expression_span: span("src/normalize.ts", 20),
                parameter_names: vec!["input".to_string()],
                returned: shape(production_canonical, 3),
                eligible: true,
                ambiguity: None,
            },
        );
        let tests = project.tests.values().cloned().collect::<Vec<_>>();
        project.assertions.clear();
        for (index, test) in tests.iter().enumerate() {
            let call = CallSiteKey {
                path: test.id.path.clone(),
                start_byte: test.id.registration_start + 30,
            };
            let argument = shape(&format!("(IdentifierReference:value{index})"), 0);
            project.call_arguments.insert(
                call.clone(),
                CallArgumentFact {
                    call: call.clone(),
                    arguments: vec![argument.clone()],
                    has_spread: false,
                },
            );
            let mut fact = assertion(test, test.id.registration_start + 20, "toEqual");
            fact.expected = Some(shape(
                &format!(
                    "(CallExpression(StaticMemberExpression(CallExpression(IdentifierReference:normalize){})(IdentifierName:trim)))",
                    argument.canonical
                ),
                3,
            ));
            fact.invoked_call = Some(call);
            fact.invokes = Resolution::Resolved(owner.clone());
            project.assertions.insert(fact.span.clone(), fact);
        }
        let findings = ImplementationMirroringTest::default().detect(&project);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].evidence[0].code, "production-return-expression");
        assert!(findings[0].explanation.contains("structurally repeat"));
    }
}
