const path = require('path');

let native;
try {
  native = require(path.join(__dirname, 'aegis-core'));
} catch {
  // Fallback: try loading from parent workspace build
  native = require(path.join(__dirname, '..', '..', 'target', 'release', 'aegis-core'));
}

const { initialize, JsAegis, JsWatchSubscription, JsTransaction } = native;

module.exports = {
  initialize,
  JsAegis,
  JsWatchSubscription,
  JsTransaction,
};
