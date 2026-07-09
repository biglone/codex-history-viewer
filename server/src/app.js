// 加载环境变量
import { readFileSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const envPath = join(__dirname, '../.env');
if (existsSync(envPath)) {
  const lines = readFileSync(envPath, 'utf8').split('\n');
  for (const line of lines) {
    const trimmed = line.trim();
    // 跳过空行和注释行
    if (!trimmed || trimmed.startsWith('#')) continue;
    // 只处理含 = 的行
    const eqIdx = trimmed.indexOf('=');
    if (eqIdx < 1) continue;
    const key = trimmed.slice(0, eqIdx).trim();
    const val = trimmed.slice(eqIdx + 1).trim();
    // 不覆盖已有环境变量（允许外部传入覆盖 .env）
    if (key && !(key in process.env)) {
      process.env[key] = val;
    }
  }
}

import Fastify from 'fastify';
import cors from '@fastify/cors';
import sessionRoutes from './routes/sessions.js';
import syncRoutes from './routes/sync.js';
import automationRoutes from './routes/automations.js';
import { mkdir } from 'fs/promises';

const PORT = parseInt(process.env.PORT || '3456');
const STORAGE_DIR = process.env.STORAGE_DIR || './storage';

const fastify = Fastify({
  // 最大请求体 50MB，应对大会话批量上传
  bodyLimit: 50 * 1024 * 1024,
  logger: {
    transport: {
      target: 'pino-pretty',
      options: { colorize: true, translateTime: 'SYS:HH:MM:ss', ignore: 'pid,hostname' },
    },
  },
});

// CORS（允许 Tauri 客户端和本地 Web 调用）
await fastify.register(cors, {
  origin: true,
  methods: ['GET', 'POST', 'PUT', 'DELETE', 'OPTIONS'],
  allowedHeaders: ['Content-Type', 'Authorization'],
});


// 确保存储目录存在
await mkdir(STORAGE_DIR, { recursive: true });

// 注册路由（统一前缀 /api）
await fastify.register(sessionRoutes, { prefix: '/api' });
await fastify.register(syncRoutes, { prefix: '/api' });
await fastify.register(automationRoutes, { prefix: '/api' });

// 根路径信息
fastify.get('/', async () => ({
  name: 'Codex Sync Server',
  version: '1.0.0',
  endpoints: {
    health:                'GET    /api/sync/health',
    stats:                 'GET    /api/sync/stats',
    upload:                'POST   /api/sessions/upload',
    uploadBatch:           'POST   /api/sessions/upload-batch',
    syncCheck:             'POST   /api/sync/check',
    syncPull:              'POST   /api/sync/pull',
    listSessions:          'GET    /api/sessions',
    searchSessions:        'GET    /api/sessions/search?q=keyword',
    getMessages:           'GET    /api/sessions/:id/messages',
    deleteSession:         'DELETE /api/sessions/:id',
    automationsUpload:     'POST   /api/automations/upload',
    automationsCheck:      'POST   /api/automations/check',
    automationsPull:       'POST   /api/automations/pull',
    automationsStats:      'GET    /api/automations/stats',
  },
}));

// 全局错误处理
fastify.setErrorHandler((error, req, reply) => {
  fastify.log.error(error);
  reply.code(error.statusCode || 500).send({
    error: error.message || 'Internal Server Error',
  });
});

// 启动
try {
  await fastify.listen({ port: PORT, host: '0.0.0.0' });
  console.log(`\n🚀  Codex Sync Server running on http://localhost:${PORT}\n`);
} catch (err) {
  fastify.log.error(err);
  process.exit(1);
}
