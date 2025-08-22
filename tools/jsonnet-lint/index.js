#!/usr/bin/env node
const fs = require('fs');
const file = process.argv[2];
if (!file) {
  console.error('usage: jsonnet-lint <file>');
  process.exit(1);
}
let data;
try {
  data = JSON.parse(fs.readFileSync(file, 'utf8'));
} catch (e) {
  console.error(`invalid JSON: ${e.message}`);
  process.exit(1);
}
function checkPanels(panels) {
  const allowed = new Set(['graph', 'stat', 'table', 'timeseries', 'gauge', 'piechart']);
  for (const p of panels || []) {
    if (!allowed.has(p.type)) {
      console.error(`unsupported panel type: ${p.type}`);
      process.exit(1);
    }
    if (p.panels) {
      checkPanels(p.panels);
    }
  }
}
if (data.panels) checkPanels(data.panels);
