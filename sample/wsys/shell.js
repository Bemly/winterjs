// WinterJS2.shell：shell 词法展开（~ 与 $VAR 过 env 门，未授权变量即拒）。
// WinterJS2.shell: shell-ish expansion (~ and $VAR pass the env gate).
// Run / 运行: winterjs2 --run sample/wsys/shell.js
console.log('[shell] home:', WinterJS2.shell.expand('$HOME') !== '$HOME');
console.log('[shell] tilde:', WinterJS2.shell.expand('~/x') !== '~/x');
console.log('[shell] plain:', WinterJS2.shell.expand('no-vars-here') === 'no-vars-here');
