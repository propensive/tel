// Tiny test driver: relies on Node's built-in `node:test` runner. Each
// imported file registers its tests via `test()` calls at module load.
// Run with `npm test` or `node run-tests.js`.

import "./base256.test.js";
import "./bintel.test.js";
