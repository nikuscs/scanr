use std::fs;
use tempfile::tempdir;

use super::types::{ProjectFacts, SlopFinding, SlopKind};
use super::{build_project_facts, collect_project_files, placement_detectors};

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

fn findings(project: &ProjectFacts, kind: SlopKind) -> Vec<SlopFinding> {
    placement_detectors::detectors()
        .into_iter()
        .find(|detector| detector.kind() == kind)
        .unwrap()
        .detect(project)
}

fn rows(finding: &SlopFinding) -> Vec<(String, String)> {
    finding.evidence.iter().map(|row| (row.label.clone(), row.span.path.clone())).collect()
}

#[test]
fn copied_type_needs_identical_code_in_two_files() {
    let shape = "{\n  id: string;\n  name: string;\n  total: number;\n}";
    let project = project(&[
        ("contract/order.ts", &format!("export interface Order {shape}\n")),
        (
            "server/order.ts",
            &format!(
                "type Order = {shape};\nexport const order: Order = {{ id: '', name: '', total: 0 }};\n"
            ),
        ),
        (
            "server/renamed.ts",
            "export interface Purchase { id: string; name: string; total: number }\n",
        ),
        (
            "server/different.ts",
            "export interface Sale { id: string; name: string; total: bigint }\n",
        ),
        ("server/short.ts", "export interface Pair { id: string; name: string }\n"),
        ("server/other.ts", "export interface Pair { id: string; name: string }\n"),
    ]);
    let found = findings(&project, SlopKind::CopiedType);
    assert_eq!(found.len(), 1, "{found:#?}");
    let mut paths = rows(&found[0]).into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths, ["contract/order.ts", "server/order.ts", "server/renamed.ts"]);
}

#[test]
fn copied_type_reports_identical_classes_but_not_same_named_tags() {
    let error = "export class OrderMissing extends Error {\n  readonly tag = 'OrderMissing';\n}\n";
    let project = project(&[
        ("contract/errors.ts", error),
        ("server/errors.ts", error),
        ("mail/mail.ts", "export class Driver extends Tag('kit/mail/Driver') {}\n"),
        ("cache/cache.ts", "export class Driver extends Tag('kit/cache/Driver') {}\n"),
    ]);
    let found = findings(&project, SlopKind::CopiedType);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].evidence[0].code, "OrderMissing");
}

#[test]
fn single_use_export_flags_helpers_and_constants_one_other_file_uses() {
    let project = project(&[
        (
            "src/order.utils.ts",
            "export function orderTotal(lines: number[]) { return lines.reduce((a, b) => a + b, 0); }\n\
             export function orderLabel(id: string) { return `#${id}`; }\n\
             export const ORDER_LIMIT = 50;\n\
             export function orderShared(id: string) { return id.trim(); }\n",
        ),
        (
            "src/order.service.ts",
            "import { orderTotal, ORDER_LIMIT, orderShared } from './order.utils';\nexport const run = () => [orderTotal([1]), ORDER_LIMIT, orderShared('a')];\n",
        ),
        (
            "src/order.query.ts",
            "import { orderLabel, orderShared } from './order.utils';\nexport const show = () => [orderLabel('1'), orderShared('b')];\n",
        ),
        (
            "src/order.test.ts",
            "import { orderShared, orderTotal } from './order.utils';\norderShared;\norderTotal([2]);\n",
        ),
    ]);
    let mut names = findings(&project, SlopKind::SingleUseExport)
        .iter()
        .map(|finding| (finding.evidence[0].code.clone(), finding.evidence[1].span.path.clone()))
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            ("ORDER_LIMIT".to_string(), "src/order.service.ts".to_string()),
            ("orderLabel".to_string(), "src/order.query.ts".to_string()),
            ("orderTotal".to_string(), "src/order.service.ts".to_string()),
        ]
    );
}

#[test]
fn single_use_export_leaves_deliberate_homes_alone() {
    let project = project(&[
        // A file with no functions is a home for values.
        (
            "src/order.constants.ts",
            "export const ORDER_LIMIT = 50;\nexport const ORDER_PAGE = 20;\n",
        ),
        // A component consumer, a value its own file also reads, and a file with one importer.
        (
            "src/order.utils.ts",
            "export function orderBadge(id: string) { return id; }\n\
             export const ORDER_GAP = 4;\n\
             export function orderGap() { return ORDER_GAP; }\n",
        ),
        ("src/solo.utils.ts", "export function soloHelper(id: string) { return id; }\n"),
        (
            "src/order-badge.tsx",
            "import { orderBadge } from './order.utils';\nexport const Badge = () => orderBadge('1');\n",
        ),
        (
            "src/order.service.ts",
            "import { ORDER_LIMIT, ORDER_PAGE } from './order.constants';\nimport { ORDER_GAP, orderGap } from './order.utils';\nimport { soloHelper } from './solo.utils';\nexport const run = () => [ORDER_LIMIT, ORDER_PAGE, ORDER_GAP, orderGap(), soloHelper('a')];\n",
        ),
        (
            "src/order.query.ts",
            "import { ORDER_LIMIT } from './order.constants';\nimport { orderGap } from './order.utils';\nexport const show = () => [ORDER_LIMIT, orderGap()];\n",
        ),
    ]);
    assert_eq!(findings(&project, SlopKind::SingleUseExport), [] as [SlopFinding; 0]);
}
