use oxc::allocator::Allocator;
use oxc::parser::Parser;
use oxc::span::SourceType;

use super::*;
use crate::scan::outline::{OutlineView, SymbolKind};
use crate::scan::types::{EXPORT_DEFAULT, EXPORT_NAMED, EXPORT_REEXPORT, LineIndex};

fn outline(source: &str) -> OutlineExtraction {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    assert!(parsed.diagnostics.is_empty(), "fixture must parse: {:?}", parsed.diagnostics);
    build_outline(&parsed.program, source, &LineIndex::new(source))
}

/// Renders the outline as indented `name kind` lines, `*` marking detail symbols.
fn shape(symbols: &[Symbol]) -> String {
    fn write(symbols: &[Symbol], depth: usize, out: &mut String) {
        for symbol in symbols {
            out.push_str(&"  ".repeat(depth));
            out.push_str(&symbol.name);
            out.push(' ');
            out.push_str(symbol.kind.as_str());
            if symbol.detail {
                out.push('*');
            }
            out.push('\n');
            write(&symbol.children, depth + 1, out);
        }
    }
    let mut out = String::new();
    write(symbols, 0, &mut out);
    out
}

fn find<'a>(symbols: &'a [Symbol], path: &str) -> &'a Symbol {
    let (head, rest) = path.split_once('.').map_or((path, None), |(head, rest)| (head, Some(rest)));
    let symbol = symbols
        .iter()
        .find(|symbol| symbol.name == head)
        .unwrap_or_else(|| panic!("no symbol named {head}"));
    rest.map_or(symbol, |rest| find(&symbol.children, rest))
}

#[test]
fn effect_service_nests_members_under_the_constructor() {
    let extraction = outline(
        r"
const make = Effect.gen(function* () {
  const db = yield* Database.Database;
  const catalogs = yield* Cache.makeWith((url: string) => http.get(url), { capacity: 1 });
  const priceOf = Effect.fnUntraced(function* ({ modelId }: Price) {
    const reported = decode(modelId);
    return reported;
  });
  const record = Effect.fn('eventsAi.record')(function* (event: Event) {
    const tokens = { cacheRead: 0 };
    yield* db.query({ build: (q) => q.insertInto('eventsAi') });
  });
  return EventsAi.of({ record });
});
export const layer = Layer.effect(EventsAi, make);
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(
        shape(symbols),
        "make const\n  db const\n  catalogs const\n  priceOf function\n    reported const*\n  \
         record function\n    tokens const*\n      cacheRead property*\nlayer const\n"
    );
    assert_eq!(find(symbols, "make").via.as_deref(), Some("Effect.gen"));
    assert_eq!(find(symbols, "make.catalogs").via.as_deref(), Some("Cache.makeWith"));
    assert_eq!(find(symbols, "make.record").via.as_deref(), Some("Effect.fn"));
    assert_eq!(find(symbols, "make.record").variable, Some(SymbolKind::Const));
    assert_eq!((find(symbols, "make").line, find(symbols, "make").end_line), (2, 14));
    assert!(find(symbols, "layer").exported);
    assert!(!find(symbols, "make").exported);
}

#[test]
fn only_recognized_wrappers_make_a_variable_a_function() {
    let extraction = outline(
        r"
const direct = () => 1;
const expression = function () { return 1; };
const memoized = memo(() => 1);
const forwarded = React.memo(forwardRef((props, ref) => null));
const callback = useCallback((event) => event, []);
const total = items.reduce((sum, item) => sum + item, 0);
const kept = Array.filter(all, (event) => event.kept);
const lazy = Effect.sync(() => 1);
let counter = 0;
var legacy = new Map();
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(
        shape(symbols),
        "direct function\nexpression function\nmemoized function\nforwarded function\n\
         callback function\ntotal const\nkept const\nlazy const\ncounter let\nlegacy var\n"
    );
    assert_eq!(find(symbols, "forwarded").via.as_deref(), Some("React.memo"));
    assert_eq!(find(symbols, "total").via.as_deref(), Some("items.reduce"));
    assert_eq!(find(symbols, "legacy").via.as_deref(), Some("new Map"));
    assert!(find(symbols, "direct").function_position.is_some());
    assert!(find(symbols, "total").function_position.is_none());
}

#[test]
fn locals_are_detail_and_named_functions_are_not() {
    let extraction = outline(
        r"
function outer() {
  const local = 1;
  const helper = () => {
    const deeper = 2;
  };
  function declared() {}
}
describe('suite', () => {
  const rows = [];
  const setup = () => {};
});
const top = 1;
",
    );

    assert_eq!(
        shape(&extraction.outline.symbols),
        "outer function\n  local const*\n  helper function\n    deeper const*\n  \
         declared function\nrows const*\nsetup function\ntop const\n"
    );
    assert_eq!(
        shape(&extraction.outline.view(OutlineView::default()).symbols),
        "outer function\n  helper function\n  declared function\nsetup function\ntop const\n"
    );
}

#[test]
fn destructuring_declares_every_bound_name() {
    let extraction = outline(
        "const { alpha, beta: renamed, ...rest } = source;\nconst [first, , third] = list;\n",
    );

    assert_eq!(
        shape(&extraction.outline.symbols),
        "alpha const\nrenamed const\nrest const\nfirst const\nthird const\n"
    );
}

#[test]
fn directly_assigned_objects_own_their_members() {
    let extraction = outline(
        r"
export const api = {
  get: () => 1,
  post() { return 2; },
  get size() { return 3; },
  nested: { deep: () => 4, VALUE: 5 },
  LIMIT: 10,
  [computed]: () => 6,
} as const;
const built = defineStore({ action() {}, handler: () => 1 });
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(
        shape(symbols),
        "api const\n  get method\n  post method\n  size getter\n  nested property*\n    \
         deep method\n    VALUE property*\n  LIMIT property*\nbuilt const\n"
    );
    assert_eq!(
        shape(&extraction.outline.view(OutlineView::default()).symbols),
        "api const\n  get method\n  post method\n  size getter\n  nested property*\n    \
         deep method\nbuilt const\n"
    );
}

#[test]
fn class_members_include_fields_and_parameter_properties() {
    let extraction = outline(
        r"
export class Service extends Context.Service<Service>()('id') {
  static readonly MAX = 3;
  #secret = 1;
  handler = () => 1;
  constructor(private readonly db: Db, plain: string) { super(); }
  run() { const inner = () => 2; return inner(); }
  get size() { return 1; }
  set size(value: number) {}
}
const Anonymous = class extends Base { method() {} };
register(class { ignored() {} });
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(
        shape(symbols),
        "Service class\n  MAX property*\n  #secret property*\n  handler method\n  db property*\n  \
         constructor constructor\n  run method\n    inner function\n  size getter\n  \
         size setter\nAnonymous class\n  method method\n"
    );
    assert_eq!(find(symbols, "Service").extends.as_deref(), Some("Context.Service"));
    assert_eq!(find(symbols, "Anonymous").extends.as_deref(), Some("Base"));
}

#[test]
fn callable_type_members_are_structure_and_data_members_are_detail() {
    let extraction = outline(
        r"
export enum Color { Red, 'Dark Green' = 2 }
export interface Shape { area(): number; area(unit: string): number; side: number; [key: string]: unknown }
export interface EventsService {
  readonly record: (event: Event) => Effect.Effect<void>;
  readonly retention: Duration;
}
type Point = { x: number; y: number; distance: (to: Point) => number };
type Alias = string;
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(
        shape(symbols),
        "Color enum\n  Red enumMember*\n  Dark Green enumMember*\nShape interface\n  \
         area method\n  side property*\nEventsService interface\n  record method\n  \
         retention property*\nPoint type\n  x property*\n  y property*\n  distance method\n\
         Alias type\n"
    );
    assert_eq!(
        shape(&extraction.outline.view(OutlineView::default()).symbols),
        "Color enum\nShape interface\n  area method\nEventsService interface\n  \
         record method\nPoint type\n  distance method\nAlias type\n"
    );
    assert_eq!(
        find(symbols, "EventsService.record").signature.as_deref(),
        Some("(event: Event) => Effect.Effect<void>")
    );
    assert!(find(symbols, "EventsService.record").function_position.is_none());
}

#[test]
fn functions_handed_out_under_another_key_record_it() {
    let extraction = outline(
        r"
const make = Effect.gen(function* () {
  const webhooks = Effect.fn('webhooks')(function* () {});
  const record = () => 1;
  const deleteUser = () => 2;
  const count = 3;
  const options = { run: record };
  schedule({ run: record });
  return Store.of({ webhookList: webhooks, record, delete: deleteUser, total: count });
});
const helper = () => 1;
export const factory = () => ({ assist: helper, helper, [dynamic]: helper });
function build() {
  const inner = () => 1;
  return { outer: inner, nested: { deep: inner } };
}
",
    );
    let symbols = &extraction.outline.symbols;
    let exposed = |path: &str| find(symbols, path).exposed_as.clone();

    assert_eq!(exposed("make.webhooks"), ["webhookList"]);
    assert_eq!(exposed("make.deleteUser"), ["delete"]);
    // Shorthand keeps the name; options passed mid-body are not handed out.
    assert_eq!(exposed("make.record"), [] as [&str; 0]);
    // Only functions are tracked: `total: count` maps data.
    assert_eq!(exposed("make.count"), [] as [&str; 0]);
    // An arrow that returns an object reaches the declarations around it.
    assert_eq!(exposed("helper"), ["assist"]);
    assert_eq!(exposed("build.inner"), ["outer"]);
}

#[test]
fn namespaces_keep_their_exports_out_of_the_module_surface() {
    let extraction = outline(
        r"
export namespace Outer { export const inside = 1; export function helper() {} }
declare module 'pkg' { interface Tables { users: string } }
declare global { interface Window { flag: boolean } }
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(
        shape(symbols),
        "Outer namespace\n  inside const\n  helper function\npkg namespace\n  Tables interface\n    \
         users property*\nglobal namespace\n  Window interface\n    flag property*\n"
    );
    assert!(find(symbols, "Outer.inside").exported);
    let exports: Vec<_> = extraction.exports.iter().map(|export| export.name.as_str()).collect();
    assert_eq!(exports, ["Outer"]);
    // Namespace members still count as exported locals for the function facts.
    assert!(extraction.exported_locals.contains("inside"));
}

#[test]
fn overloads_collapse_into_one_function() {
    let extraction = outline(
        r"
export function over(value: string): string;
export function over(value: number): number;
export function over(value: unknown) { return value; }
export declare function ambient(): void;
",
    );
    let symbols = &extraction.outline.symbols;

    assert_eq!(shape(symbols), "over function\nambient function\n");
    assert_eq!((find(symbols, "over").line, find(symbols, "over").end_line), (2, 4));
    assert!(find(symbols, "over").function_position.is_some());
    assert!(find(symbols, "ambient").function_position.is_none());
    let exports: Vec<_> = extraction.exports.iter().map(|export| export.name.as_str()).collect();
    assert_eq!(exports, ["over", "ambient"]);
}

#[test]
fn exports_report_public_names_and_reexports() {
    let extraction = outline(
        r"
import { imported, other as aliased } from './source';
import * as everything from './all';
const local = 1;
const twice = 2;
export const Schema = 1;
export type Schema = string;
export { local as renamedLocal, twice, twice as again };
export { imported, aliased as publicAlias, everything };
export * from './star';
export * as ns from './namespace';
export { one as uno, two } from './named';
export type { Shape } from './types';
",
    );
    let outline = &extraction.outline;

    assert_eq!(find(&outline.symbols, "local").exported_as, ["renamedLocal"]);
    assert_eq!(find(&outline.symbols, "twice").exported_as, ["again"]);
    assert!(find(&outline.symbols, "twice").exported);
    assert_eq!(outline.imports, ["./source", "./all"]);

    let reexports: Vec<_> = outline
        .reexports
        .iter()
        .map(|reexport| {
            format!(
                "{} <- {}{}",
                reexport.exported_names().join(","),
                reexport.from,
                if reexport.type_only { " (type)" } else { "" }
            )
        })
        .collect();
    assert_eq!(
        reexports,
        [
            "* <- ./star",
            "ns <- ./namespace",
            "uno,two <- ./named",
            "Shape <- ./types (type)",
            "imported <- ./source",
            "publicAlias <- ./source",
            "everything <- ./all",
        ]
    );
    assert_eq!(outline.reexports[2].names, ["one as uno", "two"]);
    assert_eq!(outline.reexports[5].names, ["other as publicAlias"]);
    assert_eq!(outline.reexports[6].namespace.as_deref(), Some("everything"));

    let exports: Vec<_> =
        extraction.exports.iter().map(|export| (export.name.as_str(), export.kind_code)).collect();
    assert_eq!(
        exports,
        [
            ("Schema", EXPORT_NAMED),
            ("*", EXPORT_REEXPORT),
            ("ns", EXPORT_REEXPORT),
            ("uno", EXPORT_REEXPORT),
            ("two", EXPORT_REEXPORT),
            ("Shape", EXPORT_REEXPORT),
            ("renamedLocal", EXPORT_NAMED),
            ("twice", EXPORT_NAMED),
            ("again", EXPORT_NAMED),
            ("imported", EXPORT_REEXPORT),
            ("publicAlias", EXPORT_REEXPORT),
            ("everything", EXPORT_REEXPORT),
        ]
    );
    assert!(extraction.exported_locals.contains("local"));
    assert!(!extraction.exported_locals.contains("imported"));
}

#[test]
fn default_exports_name_the_symbol_they_publish() {
    let named = outline("export default function main() {}\n");
    assert_eq!(find(&named.outline.symbols, "main").exported_as, ["default"]);
    let exports: Vec<_> =
        named.exports.iter().map(|export| (export.name.as_str(), export.kind_code)).collect();
    assert_eq!(exports, [("default", EXPORT_DEFAULT), ("main", EXPORT_DEFAULT)]);

    let anonymous = outline("export default () => 1;\n");
    let symbol = find(&anonymous.outline.symbols, "default");
    assert_eq!(symbol.kind, SymbolKind::Function);
    assert!(symbol.exported && symbol.function_position.is_some());

    let class = outline("export default class { run() {} }\n");
    assert_eq!(shape(&class.outline.symbols), "default class\n  run method\n");

    let call = outline("export default defineConfig({ plugins: [] });\n");
    let symbol = find(&call.outline.symbols, "default");
    assert_eq!((symbol.kind, symbol.via.as_deref()), (SymbolKind::Const, Some("defineConfig")));

    let object = outline("export default { fetch() {}, port: 1 };\n");
    assert_eq!(shape(&object.outline.symbols), "default const\n  fetch method\n  port property*\n");

    let identifier = outline("const app = 1;\nexport default app;\n");
    let symbol = find(&identifier.outline.symbols, "app");
    assert!(symbol.exported);
    assert_eq!(symbol.exported_as, ["default"]);
    assert_eq!(identifier.exports.len(), 1);
}

#[test]
fn callables_carry_their_signature_on_one_line() {
    let extraction = outline(
        r"
export function load<T>(id: string, options?: Options): Promise<T> { return fetch(id); }
const record = Effect.fn('record')(function* ({
  subject,
  userId,
}: Event) {});
const plain = () => 1;
class Service { run(force: boolean): void {} }
interface Shape { area(unit: string): number }
const value = 1;
",
    );
    let symbols = &extraction.outline.symbols;
    let signature = |path: &str| find(symbols, path).signature.clone();

    assert_eq!(
        signature("load").as_deref(),
        Some("<T>(id: string, options?: Options): Promise<T>")
    );
    assert_eq!(signature("record").as_deref(), Some("({ subject, userId, }: Event)"));
    assert_eq!(signature("plain").as_deref(), Some("()"));
    assert_eq!(signature("Service.run").as_deref(), Some("(force: boolean): void"));
    assert_eq!(signature("Shape.area").as_deref(), Some("(unit: string): number"));
    assert_eq!(signature("value"), None);
}
