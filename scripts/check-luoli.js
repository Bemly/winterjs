// luoli coffee 段校验（docs 部署门；CI 现装 coffeescript@2 后跑）。
// 只验 coffee: 段（保留字/括号两类已爆过的雷）；template:/style: 段是 luolita
// 预处理方言，裸 pug 验不了（缩进基不同），靠 diff 评审。
// 本地跑：先 `npm install --prefix /tmp/wjs-luoli coffeescript@2`，再
// `NODE_PATH=/tmp/wjs-luoli/node_modules node scripts/check-luoli.js`
// （本仓不进 Cargo，不污染仓库）。
const fs = require("fs");
const path = require("path");
let coffee;
try {
  coffee = require("coffeescript");
} catch {
  console.log("need coffeescript: npm install --no-save coffeescript@2 (CI) or NODE_PATH=... (local)");
  process.exit(2);
}

const root = path.join(__dirname, "..", "sample", "docs");
const files = [];
(function walk(d) {
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith(".luoli")) files.push(p);
  }
})(root);

let fail = 0;
for (const f of files) {
  const buf = [];
  let cur = null;
  for (const line of fs.readFileSync(f, "utf8").split("\n")) {
    const t = line.trimEnd();
    if (t === "coffee:" || t === "template:" || t === "style:") {
      cur = t.slice(0, -1);
      continue;
    }
    if (cur === "coffee") buf.push(line);
  }
  try {
    coffee.compile(buf.join("\n"), { bare: true, header: false });
    console.log("ok  ", path.relative(root, f));
  } catch (e) {
    fail = 1;
    console.log("FAIL", path.relative(root, f), "--", String(e.message).split("\n")[0]);
  }
}
process.exit(fail);
