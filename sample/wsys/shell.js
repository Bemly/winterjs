// WinterJS.shell：shell 词法展开（~ 与 $VAR 过 env 门，未授权变量即拒）。
// WinterJS.shell: shell-ish expansion (~ and $VAR pass the env gate).
// Run / 运行: winterjs --run sample/wsys/shell.js
console.log('[shell] home:', WinterJS.shell.expand('$HOME') !== '$HOME');
console.log('[shell] tilde:', WinterJS.shell.expand('~/x') !== '~/x');
console.log('[shell] plain:', WinterJS.shell.expand('no-vars-here') === 'no-vars-here');
