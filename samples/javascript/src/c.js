// Module c — written in CommonJS to exercise `require`, which the analyzer
// DOES detect (handled as a call to `require`).

// require() of a local file — DETECTED (c.js → b.js).
const { beta } = require("./b.js");
// require() of an external package — DETECTED → External node `chalk`.
const chalk = require("chalk");

function gamma() {
  return 3;
}

// require() with a NON-LITERAL (computed) argument — NOT detected. The analyzer
// only reads string-literal specifiers, so this dependency is invisible.
function loadPlugin(name) {
  return require("./plugins/" + name);
}

module.exports = { gamma, shout: (s) => chalk.bold(beta() + s), loadPlugin };
