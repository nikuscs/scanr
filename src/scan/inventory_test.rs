use std::fs;

use tempfile::tempdir;

use super::InventoryOutput;
use crate::scan::{ScanConfig, scan_directory};

fn inventory(files: &[(&str, &str)], include_lines: bool) -> serde_json::Value {
    let dir = tempdir().unwrap();
    for (name, source) in files {
        fs::write(dir.path().join(name), source).unwrap();
    }
    let result = scan_directory(dir.path(), &ScanConfig::default()).unwrap();
    serde_json::to_value(InventoryOutput::from_result(&result, include_lines)).unwrap()
}

fn names(list: &serde_json::Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let name = item["name"].as_str().unwrap();
            match (item.get("kind"), item.get("from")) {
                (Some(kind), _) => format!("{name}:{}", kind.as_str().unwrap()),
                (None, Some(from)) => format!("{name}<-{}", from.as_str().unwrap()),
                (None, None) => name.to_string(),
            }
        })
        .collect()
}

const CARD: &str = r"
export interface Props { title: string }
export const MAX = 3;
export const Card = memo(({ title }: Props) => {
  const label = title.trim();
  const format = () => label;
  return <section>{format()}</section>;
});
export function useCard() { return MAX; }
export class Service { run() {} handler = () => 1; }
export enum Kind { A }
let counter = 0;
";

#[test]
fn lists_every_declaration_with_its_dotted_path() {
    let v = inventory(&[("Card.tsx", CARD)], false);

    assert_eq!(
        names(&v["functions"]),
        ["Card", "Card.format", "Service.handler", "Service.run", "useCard"]
    );
    assert_eq!(
        names(&v["constants"]),
        ["Card:arrow", "Card.format:arrow", "Card.label:primitive", "MAX:primitive"]
    );
    assert_eq!(names(&v["variables"]), ["counter:let"]);
    assert_eq!(names(&v["types"]), ["Props:interface"]);
    assert_eq!(names(&v["components"]), ["Card"]);
    assert_eq!(names(&v["hooks"]), ["useCard"]);
    assert_eq!(names(&v["classes"]), ["Service"]);
    assert_eq!(names(&v["enums"]), ["Kind"]);
    assert_eq!(names(&v["exports"]), ["Card", "Kind", "MAX", "Props", "Service", "useCard"]);
    assert!(v["functions"][0].get("line").is_none());
    assert_eq!(v["functions"][0]["file"], "Card.tsx");
}

#[test]
fn includes_lines_when_requested() {
    let v = inventory(&[("Card.tsx", CARD)], true);

    assert_eq!(v["constants"][3]["name"], "MAX");
    assert_eq!(v["constants"][3]["line"], 3);
    assert_eq!(v["components"][0]["line"], 4);
    assert!(v["exports"][0].get("line").is_none());
}

#[test]
fn a_call_with_a_callback_is_a_constant_not_an_arrow() {
    let v = inventory(
        &[("a.ts", "const total = [1, 2].reduce((sum, item) => sum + item, 0);\n")],
        false,
    );

    assert_eq!(names(&v["constants"]), ["total:primitive"]);
    assert_eq!(names(&v["functions"]), [] as [&str; 0]);
}

#[test]
fn exports_are_public_names_without_duplicates() {
    let v = inventory(
        &[(
            "index.ts",
            r"
const local = 1;
export const Schema = 1;
export type Schema = number;
export function over(value: string): string;
export function over(value: unknown) { return value; }
export namespace NS { export const inside = 1; }
export interface Shape { area(): number }
export { local as renamed };
export * from './all';
export * as ns from './namespace';
export { one as uno } from './named';
",
        )],
        false,
    );

    assert_eq!(
        names(&v["exports"]),
        ["*<-./all", "NS", "Schema", "Shape", "ns<-./namespace", "over", "renamed", "uno<-./named"]
    );
    assert_eq!(names(&v["namespaces"]), ["NS"]);
    assert_eq!(
        names(&v["constants"]),
        ["NS.inside:primitive", "Schema:primitive", "local:primitive"]
    );
    // Interface method signatures are members, not functions.
    assert_eq!(names(&v["functions"]), ["over"]);
}
