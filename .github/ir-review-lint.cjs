// One-shot mechanical repair. Imports, exports and object property names stay public.
const fs = require("node:fs");
const root = process.env.IR_LINT_MODULES;
const { ESLint } = require(root + "/eslint");
const espree = require(root + "/espree");
const scope_lib = require(root + "/eslint-scope");
(async () => {
    const paths = ["src", "tests", "gen", "lib", "examples", "tools"];
    const eslint = new ESLint({ fix: true });
    const files = await eslint.lintFiles(paths);
    await ESLint.outputFixes(files);
    let renamed = 0;
    for(const result of files) {
        const complaints = result.messages.filter(m => m.ruleId === "id-match");
        if(!complaints.length) continue;
        let text = fs.readFileSync(result.filePath, "utf8");
        const ast = espree.parse(text, { ecmaVersion: "latest", sourceType: "module", range: true, loc: true });
        const parents = new WeakMap();
        function walk(node, parent) {
            if(!node || typeof node !== "object") return;
            if(node.type) parents.set(node, parent);
            for(const key of Object.keys(node)) {
                if(["range", "loc"].includes(key)) continue;
                const value = node[key];
                if(Array.isArray(value)) value.forEach(x => walk(x, node));
                else if(value && typeof value === "object") walk(value, node);
            }
        }
        walk(ast, null);
        const scopes = scope_lib.analyze(ast, { ecmaVersion: 2024, sourceType: "module", optimistic: true });
        const variables = scopes.scopes.flatMap(s => s.variables);
        const names = new Set(variables.map(v => v.name));
        const bad = new Set(complaints.map(m => `${m.line}:${m.column}`));
        const edits = new Map(), exports = new Map();
        const put = (start, end, replacement) => {
            const key = `${start}:${end}`;
            if(edits.has(key) && edits.get(key).replacement !== replacement) throw Error("conflicting edit");
            edits.set(key, {start, end, replacement});
        };
        for(const variable of variables) {
            if(!variable.identifiers.some(id => bad.has(`${id.loc.start.line}:${id.loc.start.column+1}`))) continue;
            let name = variable.name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").replace(/([A-Z])([A-Z][a-z])/g, "$1_$2").toLowerCase();
            while(names.has(name)) name += "_local";
            names.add(name);
            const ids = [...variable.identifiers, ...variable.references.map(r => r.identifier)];
            for(const id of ids) {
                const parent = parents.get(id);
                let replacement = name;
                if(parent?.type === "ImportSpecifier" && parent.local === id && parent.imported.range[0] === id.range[0]) replacement = `${variable.name} as ${name}`;
                if(parent?.type === "ExportSpecifier" && parent.local === id && parent.exported.range[0] === id.range[0]) replacement = `${name} as ${variable.name}`;
                const property = parent?.type === "AssignmentPattern" ? parents.get(parent) : parent;
                if(property?.type === "Property" && property.shorthand) replacement = `${variable.name}: ${name}`;
                put(...id.range, replacement);
            }
            for(const definition of variable.defs) {
                let declaration = definition.node;
                if(declaration?.type === "VariableDeclarator") declaration = parents.get(declaration);
                const parent = parents.get(declaration);
                if(parent?.type === "ExportNamedDeclaration") {
                    put(parent.range[0], declaration.range[0], "");
                    const key = declaration.range[1];
                    if(!exports.has(key)) exports.set(key, new Map());
                    exports.get(key).set(variable.name, name);
                }
            }
            renamed++;
        }
        for(const [end, mappings] of exports) {
            const decl = ast.body.find(n => n.type === "ExportNamedDeclaration" && n.declaration?.range[1] === end).declaration;
            const exported = decl.type === "VariableDeclaration" ? decl.declarations.map(d => d.id.name) : [decl.id.name];
            const aliases = exported.map(old => mappings.has(old) ? `${mappings.get(old)} as ${old}` : old);
            put(end, end, `\nexport { ${aliases.join(", ")} };`);
        }
        const sorted = [...edits.values()].sort((a,b) => b.start-a.start || b.end-a.end);
        let previous = text.length+1;
        for(const e of sorted) {
            if(e.end > previous) throw Error("overlap");
            previous = e.start;
            text = text.slice(0,e.start)+e.replacement+text.slice(e.end);
        }
        fs.writeFileSync(result.filePath, text);
    }
    const starter = "src/browser/starter.js";
    let text = fs.readFileSync(starter, "utf8");
    for(const name of ["configure_ir_diagnostics", "get_ir_dumps", "get_jit_info"]) {
        const line = `V86.prototype["${name}"] = V86.prototype.${name};`;
        if(!text.includes(line)) throw Error("missing Closure export");
        text = text.replace(line, "// eslint-disable-next-line no-self-assign -- Keep the public name through Closure compilation.\n" + line);
    }
    fs.writeFileSync(starter, text);
    const exchange = "tests/ir/differential/exchange.mjs";
    fs.writeFileSync(exchange, fs.readFileSync(exchange, "utf8").replaceAll("queueMicrotask(", "globalThis.queueMicrotask("));
    const fixed = await eslint.lintFiles(paths);
    await ESLint.outputFixes(fixed);
    for(const f of fixed) if(f.errorCount) { console.error(f.filePath, f.messages); process.exitCode = 1; }
    console.log(`alpha-renamed ${renamed} bindings; external property/import/export names preserved`);
})().catch(e => { console.error(e); process.exitCode = 1; });
