// WinterJS2.semver: version validation, ranges and parsing.
// WinterJS2.semver：版本号校验、区间与解析。
// Run / 运行: winterjs2 --run sample/wstd/semver.js
console.log('[semver] valid:', WinterJS2.semver.valid('1.2.3') === true);
console.log('[semver] satisfies:', WinterJS2.semver.satisfies('1.2.3', '^1.0.0') === true);
console.log('[semver] compare:', WinterJS2.semver.compare('1.0.0', '1.0.0-alpha') === 1);
console.log('[semver] parse:', JSON.stringify(WinterJS2.semver.parse('1.2.3-beta.1+build')) === '{"major":1,"minor":2,"patch":3,"pre":["beta","1"],"build":["build"]}');
