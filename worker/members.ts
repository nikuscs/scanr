import { Project, ts } from "ts-morph";

export type MembersRequest = { root: string; tsconfigs: string[]; factories: string[] };

type Ref = { file: string; line: number; type: boolean };

type Member = {
  file: string;
  factory: string;
  path: string;
  startByte: number;
  endByte: number;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
  wraps: boolean;
  refs: Ref[];
  wired: Ref[];
  handedOff: Ref[];
};

type Tracked = { member: Member; key: string; node: ts.Node; start: number; end: number; seen: Set<string> };

type FactoryObject = { node: ts.ObjectLiteralExpression; handedOff: Ref[]; members: Tracked[] };

/**
 * Resolves every property access, element access, destructuring and indexed type
 * to the members of objects returned by exported factory functions, and counts the
 * references of the function each member wraps outside the member itself.
 */
export function members(req: MembersRequest) {
  const patterns = req.factories.map(globToRegExp);
  const programs = req.tsconfigs.map(createProgram);
  const owners = new Map<string, Tracked[]>();
  const wrapped = new Map<string, Tracked[]>();
  const names = new Set<string>();
  const objects = new Map<string, FactoryObject>();
  const all: Tracked[] = [];
  const own = (key: string, entry: Tracked) => owners.set(key, [...(owners.get(key) ?? []), entry]);

  for (const program of programs) {
    const checker = program.getTypeChecker();
    for (const sf of ownFiles(program)) {
      if (!sf.fileName.startsWith(req.root)) continue;
      for (const [factory, object] of factories(sf, checker, patterns)) {
        const key = declarationKey(object);
        if (objects.has(key)) continue;
        const entry: FactoryObject = { node: object, handedOff: [], members: [] };
        objects.set(key, entry);
        collectMembers(object, factory, "", sf, checker, entry, wrapped, names);
        for (const member of entry.members) own(member.key, member);
        all.push(...entry.members);
      }
    }
  }

  // Pass 2: a factory result that flows into another declared type (an annotated return
  // type, a typed parameter or property) is read through that type, so its properties
  // count as the members. A library type (`streamText({ tools: makeTools() })`) consumes
  // the whole object, which property reads cannot follow, so the factory is handed off.
  const files = uniqueFiles(programs);
  for (const [sf, checker] of files) {
    const flow = (expression: ts.Expression) => {
      expression = unwrap(expression);
      if (ts.isAwaitExpression(expression)) expression = unwrap(expression.expression);
      if (!ts.isIdentifier(expression) && !ts.isCallExpression(expression) && !ts.isPropertyAccessExpression(expression) && !ts.isObjectLiteralExpression(expression)) return;
      const declarations = checker.getTypeAtLocation(expression).getSymbol()?.declarations ?? [];
      const object = declarations.map((declaration) => objects.get(declarationKey(declaration))).find(Boolean);
      if (object) alias(object, expression, sf, checker, own);
    };
    const visit = (node: ts.Node) => {
      if (ts.isCallExpression(node) || ts.isNewExpression(node)) (node.arguments ?? []).forEach(flow);
      else if (ts.isPropertyAssignment(node)) flow(node.initializer);
      else if (ts.isShorthandPropertyAssignment(node)) flow(node.name);
      else if (ts.isVariableDeclaration(node) && node.type && node.initializer) flow(node.initializer);
      else if (ts.isReturnStatement(node) && node.expression) flow(node.expression);
      else if (ts.isArrowFunction(node) && node.type && !ts.isBlock(node.body)) flow(node.body);
      else if (ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken) flow(node.right);
      ts.forEachChild(node, visit);
    };
    visit(sf);
  }

  for (const [sf, checker] of files) {
    const record = (symbol: ts.Symbol | undefined, node: ts.Node, type: boolean) => {
      if (!symbol) return;
      if (symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol);
      const position = node.getStart(sf);
      const ref = { file: sf.fileName, line: sf.getLineAndCharacterOfPosition(position).line + 1, type };
      for (const declaration of symbol.declarations ?? []) {
        const key = declarationKey(declaration);
        for (const owner of owners.get(key) ?? []) addRef(owner, owner.member.refs, sf.fileName, position, ref);
        for (const owner of wrapped.get(key) ?? []) addRef(owner, owner.member.wired, sf.fileName, position, ref);
      }
    };
    const visit = (node: ts.Node, type: boolean) => {
      type ||= ts.isTypeNode(node);
      if (ts.isShorthandPropertyAssignment(node) && names.has(node.name.text)) {
        record(checker.getShorthandAssignmentValueSymbol(node), node.name, type);
      } else if (ts.isIdentifier(node) && names.has(node.text) && !declares(node)) {
        record(checker.getSymbolAtLocation(node), node, type);
      } else if (ts.isElementAccessExpression(node) && ts.isStringLiteralLike(node.argumentExpression)) {
        record(checker.getSymbolAtLocation(node.argumentExpression), node.argumentExpression, type);
      } else if (ts.isBindingElement(node) && ts.isObjectBindingPattern(node.parent) && !node.dotDotDotToken) {
        const key = node.propertyName ?? node.name;
        if (ts.isIdentifier(key) || ts.isStringLiteralLike(key)) {
          record(checker.getTypeAtLocation(node.parent).getProperty(key.text), key, type);
        }
      } else if (
        ts.isIndexedAccessTypeNode(node) &&
        ts.isLiteralTypeNode(node.indexType) &&
        ts.isStringLiteralLike(node.indexType.literal)
      ) {
        record(checker.getTypeFromTypeNode(node.objectType).getProperty(node.indexType.literal.text), node.indexType, type);
      }
      ts.forEachChild(node, (child) => visit(child, type));
    };
    visit(sf, false);
  }

  return { members: all.map(({ member }) => member) };
}

/** Maps the factory's members onto the properties of the type its result flows into. */
function alias(
  object: FactoryObject,
  expression: ts.Expression,
  sf: ts.SourceFile,
  checker: ts.TypeChecker,
  own: (key: string, entry: Tracked) => void,
) {
  const contextual = checker.getContextualType(expression);
  if (!contextual || contextual.flags & (ts.TypeFlags.TypeParameter | ts.TypeFlags.Any | ts.TypeFlags.Unknown)) return;
  const target = checker.getNonNullableType(contextual);
  const declarations = [target.aliasSymbol, target.getSymbol()].flatMap((symbol) => symbol?.declarations ?? []);
  if (declarations.includes(object.node)) return;
  if (declarations.some(external)) {
    const position = expression.getStart(sf);
    object.handedOff.push({ file: sf.fileName, line: sf.getLineAndCharacterOfPosition(position).line + 1, type: false });
    return;
  }
  for (const member of object.members) {
    let type: ts.Type | undefined = target;
    let property: ts.Symbol | undefined;
    for (const segment of member.member.path.split(".")) {
      property = type && checker.getPropertyOfType(checker.getApparentType(type), segment);
      type = property && checker.getNonNullableType(checker.getTypeOfSymbol(property));
    }
    for (const declaration of property?.declarations ?? []) {
      if (declaration !== member.node) own(declarationKey(declaration), member);
    }
  }
}

function uniqueFiles(programs: ts.Program[]) {
  const seen = new Set<string>();
  const files: [ts.SourceFile, ts.TypeChecker][] = [];
  for (const program of programs) {
    const checker = program.getTypeChecker();
    for (const sf of ownFiles(program)) {
      if (seen.has(sf.fileName)) continue;
      seen.add(sf.fileName);
      files.push([sf, checker]);
    }
  }
  return files;
}

function createProgram(tsConfigFilePath: string) {
  // ts-morph serves the TypeScript lib files from memory, which a bundled compiler cannot find on disk.
  const program = new Project({ tsConfigFilePath }).getProgram().compilerObject;
  // The checker binds every file, which also sets the parent pointers the walks rely on.
  program.getTypeChecker();
  return program;
}

function ownFiles(program: ts.Program) {
  return program.getSourceFiles().filter((sf) => !sf.isDeclarationFile && !sf.fileName.includes("/node_modules/"));
}

function factories(sf: ts.SourceFile, checker: ts.TypeChecker, patterns: RegExp[]) {
  const found: [string, ts.ObjectLiteralExpression][] = [];
  const module = checker.getSymbolAtLocation(sf);
  if (!module) return found;
  for (const exported of checker.getExportsOfModule(module)) {
    const symbol = exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported;
    for (const declaration of symbol.declarations ?? []) {
      if (declaration.getSourceFile() !== sf) continue;
      const fn = ts.isVariableDeclaration(declaration) ? declaration.initializer && unwrap(declaration.initializer) : declaration;
      const name = ts.isVariableDeclaration(declaration) ? declaration.name : ts.isFunctionDeclaration(declaration) ? declaration.name : undefined;
      if (!fn || !name || !ts.isIdentifier(name) || !patterns.some((pattern) => pattern.test(name.text))) continue;
      if (!ts.isFunctionDeclaration(fn) && !ts.isArrowFunction(fn) && !ts.isFunctionExpression(fn)) continue;
      const object = returnedObject(fn.body);
      if (object) found.push([name.text, object]);
    }
  }
  return found;
}

function returnedObject(body: ts.ConciseBody | undefined) {
  if (!body) return undefined;
  if (!ts.isBlock(body)) {
    const value = unwrap(body);
    return ts.isObjectLiteralExpression(value) ? value : undefined;
  }
  const objects = body.statements.flatMap((statement) => {
    if (!ts.isReturnStatement(statement) || !statement.expression) return [];
    const value = unwrap(statement.expression);
    return ts.isObjectLiteralExpression(value) ? [value] : [];
  });
  return objects.length === 1 ? objects[0] : undefined;
}

function collectMembers(
  literal: ts.ObjectLiteralExpression,
  factory: string,
  prefix: string,
  sf: ts.SourceFile,
  checker: ts.TypeChecker,
  object: FactoryObject,
  wrapped: Map<string, Tracked[]>,
  names: Set<string>,
) {
  let callable = false;
  for (const property of literal.properties) {
    if (ts.isSpreadAssignment(property) || !property.name) continue;
    const name = property.name;
    if (!ts.isIdentifier(name) && !ts.isStringLiteralLike(name)) continue;
    const path = `${prefix}${name.text}`;
    const position = object.members.length;
    const value = ts.isPropertyAssignment(property) ? unwrap(property.initializer) : undefined;
    const keep = value && ts.isObjectLiteralExpression(value)
      ? collectMembers(value, factory, `${path}.`, sf, checker, object, wrapped, names)
      : callableType(checker.getTypeAtLocation(property.name), checker);
    if (!keep) continue;
    callable = true;
    const start = property.getStart(sf);
    const begin = sf.getLineAndCharacterOfPosition(start);
    const finish = sf.getLineAndCharacterOfPosition(property.end);
    const target = wrappedSymbol(property, checker);
    const entry: Tracked = {
      member: {
        file: sf.fileName,
        factory,
        path,
        startByte: Buffer.byteLength(sf.text.slice(0, start)),
        endByte: Buffer.byteLength(sf.text.slice(0, property.end)),
        startLine: begin.line + 1,
        startColumn: begin.character + 1,
        endLine: finish.line + 1,
        endColumn: finish.character + 1,
        wraps: target !== undefined,
        refs: [],
        wired: [],
        handedOff: object.handedOff,
      },
      key: declarationKey(property),
      node: property,
      start,
      end: property.end,
      seen: new Set(),
    };
    object.members.splice(position, 0, entry);
    names.add(name.text);
    if (target) {
      names.add(target.name);
      for (const declaration of target.declarations ?? []) {
        const targetKey = declarationKey(declaration);
        wrapped.set(targetKey, [...(wrapped.get(targetKey) ?? []), entry]);
      }
    }
  }
  return callable;
}

/**
 * Functions and objects of functions. Plain data may be serialized or read reflectively,
 * which property reads cannot follow.
 */
function callableType(type: ts.Type, checker: ts.TypeChecker) {
  const calls = (candidate: ts.Type) => checker.getNonNullableType(candidate).getCallSignatures().length > 0;
  return calls(type) || type.getProperties().some((property) => calls(checker.getTypeOfSymbol(property)));
}

/** The function or value a member forwards to: `m: (x) => fn(x)`, `m: other.fn`, `m() { return fn(); }`, `{ fn }`. */
function wrappedSymbol(property: ts.ObjectLiteralElementLike, checker: ts.TypeChecker) {
  let expression: ts.Expression | undefined;
  if (ts.isShorthandPropertyAssignment(property)) {
    return local(checker.getShorthandAssignmentValueSymbol(property), checker);
  }
  if (ts.isPropertyAssignment(property)) {
    const value = unwrap(property.initializer);
    expression = ts.isArrowFunction(value) || ts.isFunctionExpression(value) ? calledIn(value.body) : value;
  } else if (ts.isMethodDeclaration(property)) {
    expression = calledIn(property.body);
  }
  if (!expression || (!ts.isIdentifier(expression) && !ts.isPropertyAccessExpression(expression))) return undefined;
  const name = ts.isIdentifier(expression) ? expression : expression.name;
  return local(checker.getSymbolAtLocation(name), checker);
}

function calledIn(body: ts.ConciseBody | undefined) {
  if (!body) return undefined;
  let value: ts.Expression | undefined = ts.isBlock(body) ? singleReturn(body) : body;
  if (!value) return undefined;
  value = unwrap(value);
  if (ts.isAwaitExpression(value)) value = unwrap(value.expression);
  return ts.isCallExpression(value) ? unwrap(value.expression) : undefined;
}

function singleReturn(block: ts.Block) {
  const [statement] = block.statements;
  if (block.statements.length !== 1 || !statement) return undefined;
  if (ts.isReturnStatement(statement)) return statement.expression;
  return ts.isExpressionStatement(statement) ? statement.expression : undefined;
}

function local(symbol: ts.Symbol | undefined, checker: ts.TypeChecker) {
  if (symbol && symbol.flags & ts.SymbolFlags.Alias) symbol = checker.getAliasedSymbol(symbol);
  const declarations = symbol?.declarations ?? [];
  return declarations.length > 0 && !declarations.some(external) ? symbol : undefined;
}

function external(declaration: ts.Node) {
  const sf = declaration.getSourceFile();
  return sf.isDeclarationFile || sf.fileName.includes("/node_modules/");
}

function addRef(owner: Tracked, refs: Ref[], file: string, position: number, ref: Ref) {
  if (file === owner.member.file && position >= owner.start && position < owner.end) return;
  const key = `${refs === owner.member.refs ? "r" : "w"}:${file}:${position}`;
  if (owner.seen.has(key)) return;
  owner.seen.add(key);
  refs.push(ref);
}

function declares(identifier: ts.Identifier) {
  const parent = identifier.parent;
  if (ts.isImportSpecifier(parent) || ts.isExportSpecifier(parent) || ts.isImportClause(parent) || ts.isNamespaceImport(parent)) {
    return true;
  }
  return declarationKinds.has(parent.kind) && (parent as ts.NamedDeclaration).name === identifier;
}

const declarationKinds = new Set([
  ts.SyntaxKind.VariableDeclaration,
  ts.SyntaxKind.Parameter,
  ts.SyntaxKind.BindingElement,
  ts.SyntaxKind.FunctionDeclaration,
  ts.SyntaxKind.FunctionExpression,
  ts.SyntaxKind.ClassDeclaration,
  ts.SyntaxKind.ClassExpression,
  ts.SyntaxKind.InterfaceDeclaration,
  ts.SyntaxKind.TypeAliasDeclaration,
  ts.SyntaxKind.EnumDeclaration,
  ts.SyntaxKind.EnumMember,
  ts.SyntaxKind.ModuleDeclaration,
  ts.SyntaxKind.TypeParameter,
  ts.SyntaxKind.PropertyAssignment,
  ts.SyntaxKind.PropertyDeclaration,
  ts.SyntaxKind.PropertySignature,
  ts.SyntaxKind.MethodDeclaration,
  ts.SyntaxKind.MethodSignature,
  ts.SyntaxKind.GetAccessor,
  ts.SyntaxKind.SetAccessor,
]);

function declarationKey(node: ts.Node) {
  return `${node.getSourceFile().fileName}:${node.getStart()}`;
}

function unwrap(expression: ts.Expression): ts.Expression {
  while (
    ts.isParenthesizedExpression(expression) ||
    ts.isAsExpression(expression) ||
    ts.isSatisfiesExpression(expression) ||
    ts.isNonNullExpression(expression)
  ) {
    expression = expression.expression;
  }
  return expression;
}

function globToRegExp(glob: string) {
  const source = glob.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  return new RegExp(`^${source}$`);
}
