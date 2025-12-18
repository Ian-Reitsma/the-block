const fs = require('fs');
const schema = {
  openapi: "3.0.0",
  info: { title: "metal-orchard API", version: "0.0.1" },
  paths: {}
};
fs.writeFileSync('openapi.json', JSON.stringify(schema, null, 2));
