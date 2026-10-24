use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::member_detectors::{self, ResolvedMember, ResolvedRef, type_aware_findings};
use super::types::{ProjectFacts, SlopKind, SlopOptions};
use super::{build_project_facts, collect_project_files};

fn project(files: &[(&str, &str)]) -> ProjectFacts {
    let root = tempdir().unwrap();
    for (path, source) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let root = root.path().canonicalize().unwrap();
    build_project_facts(&root, collect_project_files(&root).unwrap()).unwrap()
}

fn unused(project: &ProjectFacts, factories: &[&str]) -> Vec<String> {
    let options = SlopOptions {
        member_factories: factories.iter().map(ToString::to_string).collect(),
        ..SlopOptions::default()
    };
    member_detectors::detectors(&options)
        .into_iter()
        .flat_map(|detector| detector.detect(project))
        .map(|finding| finding.evidence[0].detail.split('`').nth(3).unwrap().to_string())
        .collect()
}

const SERVICE: &str = r"
export function makeRedisService(prefix: string) {
  return {
    increment: (key: string) => `${prefix}${key}`,
    decrement(key: string): string { return key ? this.decrement(key.slice(1)) : key; },
    locker: (key: string) => key,
    funnel: (key: string) => key,
    flexible: (key: string) => key,
    connect: () => true,
    clear: (pattern = 'cache:*') => pattern,
    actions: {
      profile: { remember: (id: string) => id },
      details: { save: (id: string) => id },
    },
    'quoted-key': () => 1,
    ttl: 60,
    limits: { max: 10 },
    close() { return true; },
  };
}
";

#[test]
fn reports_members_no_file_reads_by_any_access_shape() {
    let project = project(&[
        ("src/redis.service.ts", SERVICE),
        (
            "src/app.tsx",
            r"
import { makeRedisService } from './redis.service';
const redis = makeRedisService('app:');
redis.increment('a');
redis?.locker('a');
redis['clear']();
const { funnel } = redis;
const { flexible: run } = redis;
export const connect = ({ connect }: typeof redis) => connect();
const { actions: { details } } = redis;
details.save('1');
export const View = () => <redis.close />;
",
        ),
    ]);
    assert_eq!(
        unused(&project, &["make*"]),
        ["decrement", "actions.profile.remember", "quoted-key"]
    );
}

#[test]
fn factory_globs_exports_tests_and_whole_object_hand_offs_limit_the_scan() {
    let project = project(&[
        ("src/redis.service.ts", SERVICE),
        (
            "src/app.ts",
            "import { makeRedisService } from './redis.service';\nmakeRedisService('a').increment('a');\n",
        ),
        (
            "src/tools.ts",
            "export const makeTools = () => ({ search_products: () => 1, list_orders: () => 2 });\nexport function makeHandlers() { return { onOpen: () => 1, onClose: () => 2 }; }\nfunction makeLocal() { return { hidden: () => 1 }; }\nmakeLocal();\n",
        ),
        (
            "src/chat.ts",
            "import { makeHandlers } from './tools';\nstreamText({ tools: makeTools(), handlers: makeHandlers() });\nmakeHandlers().onOpen();\n",
        ),
        (
            "tests/fixtures.ts",
            "export function makeFixture() { return { unused: () => 1, used: () => 2 }; }\nmakeFixture().used();\n",
        ),
    ]);
    assert_eq!(unused(&project, &["make*Service"]), unused(&project, &["make*"]));
    assert_eq!(
        unused(&project, &["makeTools", "makeHandlers", "makeLocal", "makeFixture"]),
        [] as [String; 0]
    );
}

fn reference(file: &str, line: u32, type_only: bool) -> ResolvedRef {
    ResolvedRef { file: format!("/repo/{file}"), line, type_only }
}

fn member(
    path: &str,
    wraps: bool,
    refs: Vec<ResolvedRef>,
    wired: Vec<ResolvedRef>,
) -> ResolvedMember {
    ResolvedMember {
        file: "/repo/src/cart.service.ts".to_string(),
        factory: "makeCartService".to_string(),
        path: path.to_string(),
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        start_column: 1,
        end_line: 1,
        end_column: 2,
        wraps,
        refs,
        wired,
        handed_off: Vec::new(),
    }
}

#[test]
fn type_aware_classifies_unused_test_only_and_exposure_only_members() {
    let prod = || reference("src/app.ts", 3, false);
    let test = || reference("tests/cart.test.ts", 9, false);
    let members = [
        member("queries", false, vec![prod()], Vec::new()),
        member("decrement", true, Vec::new(), Vec::new()),
        member("cleanup", true, vec![test()], vec![test()]),
        member("validation", true, vec![reference("src/types.ts", 4, true), test()], vec![prod()]),
        member("actions", false, vec![prod()], Vec::new()),
        member("actions.pay", true, vec![prod()], Vec::new()),
        member("actions.profile", false, Vec::new(), Vec::new()),
        member("actions.profile.remember", true, Vec::new(), Vec::new()),
        member("options", false, vec![test()], Vec::new()),
        member("options.limit", false, Vec::new(), Vec::new()),
    ];
    let findings = type_aware_findings(Path::new("/repo"), &members)
        .into_iter()
        .map(|finding| {
            (finding.kind, finding.evidence[0].detail.split('`').nth(3).unwrap().to_string())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        findings,
        [
            (SlopKind::UnusedFactoryMember, "decrement".to_string()),
            (SlopKind::TestOnlyFactoryMember, "cleanup".to_string()),
            (SlopKind::ExposureOnlyMember, "validation".to_string()),
            (SlopKind::UnusedFactoryMember, "actions.profile.remember".to_string()),
            (SlopKind::TestOnlyFactoryMember, "options".to_string()),
        ]
    );
    let detail = &type_aware_findings(Path::new("/repo"), &members[3..4])[0].evidence[0].detail;
    assert!(detail.contains("(1 type-only), 1 test; the wrapped function has 1 production reference elsewhere: src/app.ts:3."), "{detail}");
}

#[test]
fn type_aware_skips_test_factories_and_production_hand_offs() {
    let mut handed = member("tools", false, Vec::new(), Vec::new());
    handed.handed_off = vec![reference("src/chat.ts", 2, false)];
    let mut handed_in_test = member("mock", false, Vec::new(), Vec::new());
    handed_in_test.handed_off = vec![reference("tests/chat.test.ts", 2, false)];
    let mut fixture = member("fixture", false, Vec::new(), Vec::new());
    fixture.file = "/repo/tests/support/auth.ts".to_string();
    let findings = type_aware_findings(Path::new("/repo"), &[handed, handed_in_test, fixture]);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].span.path, "src/cart.service.ts");
    assert!(findings[0].evidence[0].detail.contains("`mock`"));
}
