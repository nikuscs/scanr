import { Project, SyntaxKind } from "ts-morph";
import { fail } from "./protocol";

export type RenameRequest = { tsconfig: string; file: string; line: number; name: string; newName: string; dryRun: boolean };

export function rename(req: RenameRequest) {
  const project = new Project({ tsConfigFilePath: req.tsconfig });
  const sf = project.getSourceFile(req.file) ?? project.addSourceFileAtPath(req.file);
  const id = sf
    .getDescendantsOfKind(SyntaxKind.Identifier)
    .find((n) => n.getText() === req.name && n.getStartLineNumber() === req.line);
  if (!id) fail(`no identifier "${req.name}" on line ${req.line} of ${req.file}`);
  id.rename(req.newName, { renameInStrings: false, renameInComments: false, usePrefixAndSuffixText: true });
  const files = project.getSourceFiles().filter((f) => !f.isSaved()).map((f) => f.getFilePath()).sort();
  if (!req.dryRun) project.saveSync();
  return { dryRun: req.dryRun, files };
}
