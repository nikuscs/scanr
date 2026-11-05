import { members } from "./members";
import { fail, reply } from "./protocol";
import { rename } from "./rename";

try {
  const { command, ...req } = JSON.parse(process.env.SCANR_WORKER_REQUEST ?? "");
  if (command === "rename") reply(rename(req));
  else if (command === "members") reply(members(req));
  else fail(`unknown worker command "${command}"`);
} catch (error) {
  fail(error instanceof Error ? error.message : String(error));
}
