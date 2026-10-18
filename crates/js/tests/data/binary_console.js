// console.log of ArrayBuffer and DataView objects (M7 feature 11a); the
// expected output is that of Node.js 22 (`node binary_console.js`).
function bytes(values, max) {
  const b = max === undefined ? new ArrayBuffer(values.length) : new ArrayBuffer(values.length, { maxByteLength: max });
  const d = new DataView(b);
  values.forEach((v, i) => d.setUint8(i, v));
  return b;
}
console.log(new ArrayBuffer(2));
console.log(new ArrayBuffer(0));
console.log(bytes([1, 0xab, 16]));
console.log(bytes([1, 2, 3], 10));
const full = new ArrayBuffer(100);
new DataView(full).setUint32(96, 0xffffffff);
console.log(full);
console.log(new ArrayBuffer(101));
console.log(new ArrayBuffer(150));
console.log(new ArrayBuffer(51));
console.log(new ArrayBuffer(9));
console.log(new ArrayBuffer(8));
const detached = new ArrayBuffer(4);
const moved = detached.transfer();
console.log(detached, moved.byteLength);
console.log(new DataView(new ArrayBuffer(4), 1));
console.log(new DataView(new ArrayBuffer(6)));
console.log(new DataView(new ArrayBuffer(7)));
console.log(new DataView(bytes([1, 2, 3], 8), 1));
console.log([new ArrayBuffer(1)], { a: new DataView(new ArrayBuffer(1)) });
console.log({ buffer: new ArrayBuffer(8) });
console.log({ x: { y: { z: new ArrayBuffer(2) } } });
console.log([[[new ArrayBuffer(2), new DataView(new ArrayBuffer(1))]]]);
class X extends ArrayBuffer {}
console.log(new X(1));
class Y extends DataView {}
console.log(new Y(new ArrayBuffer(1)));
const withProps = new ArrayBuffer(1);
withProps.foo = 1;
console.log(withProps);
const viewProps = new DataView(new ArrayBuffer(2));
viewProps.x = 5;
console.log(viewProps);
console.log(ArrayBuffer.prototype, DataView.prototype);
console.log(String(new ArrayBuffer(1)), Object.prototype.toString.call(new DataView(new ArrayBuffer(1))));
console.log(ArrayBuffer, DataView, ArrayBuffer.isView);
const shared = new ArrayBuffer(3);
console.log([shared, shared]);
