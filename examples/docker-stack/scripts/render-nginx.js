const fs = require('fs');
const path = require('path');
const Synx = require('@aperturesyndicate/synx-format');

const root = __dirname ? path.resolve(__dirname, '..') : process.cwd();
const configPath = path.join(root, 'config', 'app.synx');
const templatePath = path.join(root, 'nginx', 'default.conf.template');
const outDir = path.join(root, 'generated');
const outPath = path.join(outDir, 'default.conf');

const cfg = Synx.loadSync(configPath, {
  basePath: path.dirname(configPath),
  env: process.env,
  strict: true,
});

const template = fs.readFileSync(templatePath, 'utf-8');
const rendered = template
  .replace('{{NGINX_PORT}}', String(cfg.nginx.listen))
  .replace('{{APP_HOST}}', String(cfg.nginx.upstream_host))
  .replace('{{APP_PORT}}', String(cfg.nginx.upstream_port));

fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(outPath, rendered, 'utf-8');

console.log('[synx] nginx config generated:', outPath);
console.log('[synx] app endpoint:', `${cfg.app.host}:${cfg.app.port}`);
console.log('[synx] redis url:', cfg.redis.url);
