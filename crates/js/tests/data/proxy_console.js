// console.log of proxies: the target is shown, no trap runs (the expected
// output is that of Node.js 22, `node proxy_console.js`). A proxy of a
// proxy is left out: Node.js unwraps one level and runs the traps of the
// inner proxy; swb shows the innermost target.
const log = [];
const h = new Proxy({}, { get(_, k) { log.push(String(k)); return undefined; } });
console.log(new Proxy({ a: 1, b: [1, 2] }, h));
console.log(new Proxy([1, 2, 3], h));
console.log(new Proxy(function f() {}, h));
console.log(new Proxy(class A {}, h));
console.log(new Proxy(new Map([[1, 2]]), h));
const r = Proxy.revocable({}, {});
r.revoke();
console.log(r.proxy);
const r2 = Proxy.revocable(function g() {}, {});
r2.revoke();
console.log(r2.proxy);
console.log([new Proxy({ a: 1 }, h)]);
console.log({ p: new Proxy([1], h) });
const self = { name: 'self' };
self.me = new Proxy(self, h);
console.log(self);
console.log('traps:', log.length);
