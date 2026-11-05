use std::fs;
use std::path::Path;
use std::process::{Command as StdCommand, Stdio};

use assert_cmd::Command;
use tempfile::tempdir;

fn js_runtime_available() -> bool {
    ["bun", "node"].iter().any(|candidate| {
        StdCommand::new(candidate)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// Service factories shaped like a real dead-code report: dead and test-only members,
/// nested `actions` paths, destructuring, functions wired twice, type-only references,
/// interface-typed factories and a factory handed whole to a library.
const SERVICES: &[(&str, &str)] = &[
    (
        "tsconfig.json",
        r#"{"compilerOptions":{"strict":true,"target":"es2022","module":"esnext","moduleResolution":"bundler","noEmit":true},"include":["src","tests"]}"#,
    ),
    (
        "node_modules/fake-ai/index.d.ts",
        "export type ToolSet = Record<string, { run(): string }>;\nexport function streamText(options: { tools: ToolSet }): void;\n",
    ),
    ("node_modules/fake-ai/package.json", r#"{"name":"fake-ai","types":"index.d.ts"}"#),
    (
        "src/redis/redis-action.decrement.ts",
        "export const redisActionDecrement = (key: string) => `${key}-1`;\n",
    ),
    (
        "src/redis/redis-action.funnel.ts",
        "export const redisActionFunnel = (key: string) => key;\n",
    ),
    (
        "src/redis/redis.service.ts",
        r"import { redisActionDecrement } from './redis-action.decrement';
import { redisActionFunnel } from './redis-action.funnel';

export function makeRedisService(prefix: string) {
  return {
    prefix,
    increment: (key: string) => `${prefix}${key}+1`,
    decrement: (key: string) => redisActionDecrement(key),
    funnel: (key: string) => redisActionFunnel(key),
  };
}

export type RedisService = ReturnType<typeof makeRedisService>;
",
    ),
    (
        "src/cart/cart.service.ts",
        r"export const cartActionResolve = (id: string) => id;
export const cartActionComplete = (id: string) => id;

export function makeCartService() {
  const steps = { resolve: (id: string) => cartActionResolve(id) };
  const completion = { complete: (id: string) => cartActionComplete(id) };
  const queries = { checkout: (id: string) => steps.resolve(id) };

  return {
    queries,
    actions: {
      cleanup: () => 'cleaned',
      resolve: steps.resolve,
      pay: (id: string) => id,
      profile: {
        remember: (id: string) => id,
      },
      completion: {
        complete: completion.complete,
        handlePaymentCompleted: (id: string) => completion.complete(id),
      },
    },
  };
}
",
    ),
    (
        "src/ai/ai.service.ts",
        r"export const aiActionAnalyze = (text: string) => text.length;
export const aiActionText = (text: string) => text.trim();

export function makeAiService() {
  return {
    actions: {
      enrich: (text: string) => aiActionAnalyze(aiActionText(text)),
      media: { analyze: (text: string) => aiActionAnalyze(text) },
      text: (text: string) => aiActionText(text),
    },
  };
}
",
    ),
    (
        "src/chat/chat.service.ts",
        r"export const chatActionAppendMessage = (text: string) => text;
export const chatActionEnsureConversation = (id: string) => id;
const chatActionHandle = (deps: { append: (text: string) => string; ensure: (id: string) => string }, text: string) =>
  deps.append(deps.ensure(text));

export function makeChatService() {
  return {
    actions: {
      ensureConversation: (id: string) => chatActionEnsureConversation(id),
      appendMessage: (text: string) => chatActionAppendMessage(text),
      handle: (text: string) =>
        chatActionHandle({ append: (input) => chatActionAppendMessage(input), ensure: chatActionEnsureConversation }, text),
    },
  };
}
",
    ),
    (
        "src/storage/storage-action.put.ts",
        "export const storageActionPut = (key: string) => key;\n",
    ),
    (
        "src/storage/storage-action.avatar.ts",
        "import { storageActionPut } from './storage-action.put';\nexport const storageActionAvatar = (key: string) => storageActionPut(`avatar/${key}`);\n",
    ),
    (
        "src/storage/storage.service.ts",
        r"import { storageActionAvatar } from './storage-action.avatar';
import { storageActionPut } from './storage-action.put';

export function makeStorageService() {
  return {
    put: (key: string) => storageActionPut(key),
    putMany: (keys: string[]) => keys.map((key) => key),
    destroy: (key: string) => key,
    replaceMany: (keys: string[]) => keys,
    urls: (keys: string[]) => keys,
    avatar: (key: string) => storageActionAvatar(key),
    async health() {
      return true;
    },
  };
}

export type StorageService = ReturnType<typeof makeStorageService>;
",
    ),
    (
        "src/order/order.service.ts",
        r"export const orderValidationFields = (value: string) => value.length > 0;

export function makeInstagramOrderService() {
  return {
    queries: { list: () => [] as string[] },
    options: () => ['a'],
    validation: { fields: (value: string) => orderValidationFields(value) },
  };
}

export type InstagramOrderService = ReturnType<typeof makeInstagramOrderService>;
export type InstagramValidation = InstagramOrderService['validation'];
",
    ),
    (
        "src/events-ai/events-ai.service.ts",
        r"export function makeEventsAiService() {
  const queries = { summary: () => 1 };
  const actions = { recordBatch: () => 2 };
  return { queries, actions };
}
",
    ),
    (
        "src/payment/payment-gateway.ts",
        r"export interface PaymentGateway {
  key: string;
  create(amount: number): number;
}

export function makeOnchainGateway(): PaymentGateway {
  return {
    key: 'onchain',
    create: (amount: number) => amount,
  };
}
",
    ),
    (
        "src/chat/chat-tools.ts",
        r"export function makeChatTools() {
  return {
    search_products: { run: () => 'products' },
    list_orders: { run: () => 'orders' },
  };
}
",
    ),
    (
        "src/core.services.ts",
        r"import { streamText } from 'fake-ai';
import { makeAiService } from './ai/ai.service';
import { makeCartService } from './cart/cart.service';
import { makeChatService } from './chat/chat.service';
import { makeChatTools } from './chat/chat-tools';
import { makeEventsAiService } from './events-ai/events-ai.service';
import { makeInstagramOrderService, orderValidationFields } from './order/order.service';
import { makeOnchainGateway, type PaymentGateway } from './payment/payment-gateway';
import { makeRedisService } from './redis/redis.service';
import { makeStorageService, type StorageService } from './storage/storage.service';

export function makeServices() {
  return {
    ai: makeAiService(),
    cart: makeCartService(),
    chat: makeChatService(),
    eventsAi: makeEventsAiService(),
    instagramOrder: makeInstagramOrderService(),
    redis: makeRedisService('app:'),
    storage: makeStorageService(),
  };
}

export type Services = ReturnType<typeof makeServices>;

const charge = (gateway: PaymentGateway) => `${gateway.key}:${gateway.create(1)}`;
charge(makeOnchainGateway());

streamText({ tools: makeChatTools() });

export function wire(services: Services) {
  services.redis.increment('a');
  console.log(services.redis.prefix);
  const { cart } = services;
  cart.actions.pay('1');
  cart?.actions.completion.handlePaymentCompleted('1');
  cart.queries.checkout('1');
  services.ai.actions.enrich('x');
  services.chat.actions['handle']('x');
  services.eventsAi.actions.recordBatch();
  services.instagramOrder.queries.list();
  orderValidationFields('direct');
  const upload = ({ avatar }: StorageService) => avatar('me');
  return upload(services.storage);
}
",
    ),
    (
        "tests/services.test.ts",
        r"import { makeServices } from '../src/core.services';

const { redis, cart, storage, chat, instagramOrder, eventsAi } = makeServices();
redis.funnel('k');
cart.actions.cleanup();
cart.actions.resolve('1');
cart.actions.completion.complete('1');
storage.put('a');
storage.putMany(['a']);
storage.destroy('a');
storage.replaceMany(['a']);
storage.urls(['a']);
await storage.health();
chat.actions.appendMessage('x');
chat.actions.ensureConversation('x');
const { options: listOptions } = instagramOrder;
listOptions();
instagramOrder.validation.fields('x');
eventsAi.queries.summary();
",
    ),
];

#[test]
fn type_aware_members_match_the_dead_code_report_shapes() {
    if !js_runtime_available() {
        return;
    }
    let root = tempdir().unwrap();
    for (path, source) in SERVICES {
        write(root.path(), path, source);
    }
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .args([
            "slop",
            "--root",
            root.path().to_str().unwrap(),
            "--type-aware",
            "--only",
            "unusedFactoryMember,testOnlyFactoryMember,exposureOnlyMember",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut found = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            let detail = finding["evidence"][0]["detail"].as_str().unwrap();
            let member = detail.split('`').nth(3).unwrap();
            format!("{} {member}", finding["kind"].as_str().unwrap())
        })
        .collect::<Vec<_>>();
    found.sort();
    let mut expected = vec![
        "unusedFactoryMember decrement",
        "testOnlyFactoryMember funnel",
        "unusedFactoryMember actions.profile.remember",
        "testOnlyFactoryMember actions.cleanup",
        "exposureOnlyMember actions.resolve",
        "exposureOnlyMember actions.completion.complete",
        "exposureOnlyMember actions.media.analyze",
        "exposureOnlyMember actions.text",
        "exposureOnlyMember actions.appendMessage",
        "exposureOnlyMember actions.ensureConversation",
        "exposureOnlyMember put",
        "testOnlyFactoryMember putMany",
        "testOnlyFactoryMember destroy",
        "testOnlyFactoryMember replaceMany",
        "testOnlyFactoryMember urls",
        "testOnlyFactoryMember health",
        "testOnlyFactoryMember options",
        "exposureOnlyMember validation",
        "testOnlyFactoryMember queries",
    ];
    expected.sort_unstable();
    assert_eq!(found, expected, "{}", String::from_utf8_lossy(&output.stdout));
}

/// A project with one dead and one used member, and a `bun` on PATH that answers the members
/// worker with `reply` and `exit`, the way a crashed or partly failed worker does.
#[cfg(unix)]
fn run_with_fake_worker(reply: impl Fn(&Path) -> String, exit: i32) -> (serde_json::Value, String) {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    write(root.path(), "tsconfig.json", r#"{"include":["src"]}"#);
    write(
        root.path(),
        "src/cart.service.ts",
        "export function makeCartService() {\n  return { pay: () => 1, refund: () => 2 };\n}\nmakeCartService().pay();\n",
    );
    let bin = tempdir().unwrap();
    let canonical = root.path().canonicalize().unwrap();
    fs::write(bin.path().join("reply.txt"), reply(&canonical)).unwrap();
    let script = format!(
        "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\ncat >/dev/null\ncat '{}'\necho 'error: Debug Failure. False expression: parameter should have errors when reporting errors' >&2\nexit {exit}\n",
        bin.path().join("reply.txt").display()
    );
    let bun = bin.path().join("bun");
    fs::write(&bun, script).unwrap();
    fs::set_permissions(&bun, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.path().display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::cargo_bin("scanr")
        .unwrap()
        .env("PATH", path)
        .args(["slop", "--root", root.path().to_str().unwrap(), "--type-aware", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let members = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            let detail = finding["evidence"][0]["detail"].as_str().unwrap();
            format!("{} {}", finding["kind"].as_str().unwrap(), detail.split('`').nth(3).unwrap())
        })
        .collect::<Vec<_>>()
        .join(", ");
    (report, members)
}

#[cfg(unix)]
#[test]
fn a_crashed_type_aware_worker_keeps_the_rest_of_the_run() {
    let (report, members) = run_with_fake_worker(|_| String::new(), 1);
    assert_eq!(members, "unusedFactoryMember refund");
    let warnings = report["warnings"].to_string();
    assert!(warnings.contains("type-aware members skipped"), "{warnings}");
    assert!(warnings.contains("error: Debug Failure."), "{warnings}");
}

#[cfg(unix)]
#[test]
fn a_checker_failure_on_one_file_skips_only_the_members_it_could_have_read() {
    let reply = |root: &Path| {
        let file = root.join("src/cart.service.ts").display().to_string();
        let member = |path: &str, unresolved: bool| {
            serde_json::json!({
                "file": file, "factory": "makeCartService", "path": path,
                "startByte": 0, "endByte": 1, "startLine": 2, "startColumn": 12, "endLine": 2, "endColumn": 13,
                "wraps": false, "refs": [], "wired": [], "handedOff": [], "unresolved": unresolved,
            })
        };
        serde_json::json!({
            "ver": 1,
            "status": "ok",
            "members": [member("pay", true), member("refund", false)],
            "failures": [{ "file": root.join("src/ai.ts").display().to_string(), "line": 26, "message": "Debug Failure." }],
        })
        .to_string()
    };
    let (report, members) = run_with_fake_worker(reply, 0);
    assert_eq!(members, "unusedFactoryMember refund");
    assert_eq!(
        report["warnings"][0].as_str().unwrap(),
        "type-aware members: the TypeScript checker failed at src/ai.ts:26, so members it could have read are not reported: Debug Failure."
    );
}
