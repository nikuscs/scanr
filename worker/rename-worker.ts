import { Project, SyntaxKind } from "ts-morph";

const req = JSON.parse(process.env.SCANR_RENAME_REQUEST ?? "");
const fail = (message: string): never => {
  console.log(JSON.stringify({ ver: 1, status: "error", message }));
  process.exit(1);
};
const project = new Project({ tsConfigFilePath: req.tsconfig });
const sf = project.getSourceFile(req.file) ?? project.addSourceFileAtPath(req.file);
const id = sf
  .getDescendantsOfKind(SyntaxKind.Identifier)
  .find((n) => n.getText() === req.name && n.getStartLineNumber() === req.line);
if (!id) fail(`no identifier "${req.name}" on line ${req.line} of ${req.file}`);
id.rename(req.newName, { renameInStrings: false, renameInComments: false, usePrefixAndSuffixText: true });
const files = project.getSourceFiles().filter((f) => !f.isSaved()).map((f) => f.getFilePath()).sort();
if (!req.dryRun) project.saveSync();
console.log(JSON.stringify({ ver: 1, status: "ok", dryRun: req.dryRun, files }));
