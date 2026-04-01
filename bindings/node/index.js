'use strict';
/**
 * SYNX Node.js native binding loader.
 * Cargo builds synx_node.dll — Node.js requires a .node extension.
 * This loader copies it once, then requires it.
 */
const path = require('path');
const fs   = require('fs');

const releasePaths = [
    path.join(__dirname, 'target', 'release'),
    path.resolve(__dirname, '..', '..', 'target', 'release'),
];

function loadBinding() {
    const tried = [];

    for (const releasePath of releasePaths) {
        const dllFile = path.join(releasePath, 'synx_node.dll');
        const nodeFile = path.join(releasePath, 'synx_node.node');

        tried.push(releasePath);

        if (!fs.existsSync(nodeFile) && fs.existsSync(dllFile)) {
            fs.mkdirSync(releasePath, { recursive: true });
            fs.copyFileSync(dllFile, nodeFile);
        }

        if (fs.existsSync(nodeFile)) {
            return require(nodeFile);
        }
    }

    throw new Error(
        '[synx-node] Native binding not built.\n' +
        'Searched: ' + tried.join(', ') + '\n' +
        'Run: cargo build --release -p synx-node'
    );
}

module.exports = loadBinding();
