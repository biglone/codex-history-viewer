// 数据库初始化脚本：读取 schema.sql 并执行
// 用法: node src/db/init.js

import { readFileSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import pg from 'pg';

const { Client } = pg;
const __dirname = dirname(fileURLToPath(import.meta.url));

// 支持从环境变量或命令行读取 DATABASE_URL
const DATABASE_URL = process.env.DATABASE_URL || 'postgresql://codex:codex123@localhost:5432/codex_sync';

async function init() {
  const client = new Client({ connectionString: DATABASE_URL });
  await client.connect();
  console.log('[DB] Connected to PostgreSQL');

  const schema = readFileSync(join(__dirname, 'schema.sql'), 'utf8');
  await client.query(schema);
  console.log('[DB] Schema initialized successfully');

  await client.end();
  console.log('[DB] Done ✓');
}

init().catch((err) => {
  console.error('[DB] Init failed:', err.message);
  process.exit(1);
});
