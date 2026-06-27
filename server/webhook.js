#!/usr/bin/env node
/**
 * GitHub Push Webhook 自动部署服务
 * 监听 GitHub push 事件 → git pull → docker compose up -d --build
 *
 * 环境变量（从 .env 读取）：
 *   WEBHOOK_SECRET   GitHub Webhook Secret（用于验证签名）
 *   WEBHOOK_PORT     监听端口（默认 9456）
 *   REPO_DIR         仓库目录（默认脚本所在目录的上级）
 *   DEPLOY_BRANCH    触发部署的分支（默认 main）
 */

import { createHmac, timingSafeEqual } from 'crypto';
import { createServer } from 'http';
import { exec } from 'child_process';
import { readFileSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));

// ── 加载 .env ──────────────────────────────────────────────────────────────
const envPath = join(__dirname, '.env');
if (existsSync(envPath)) {
  for (const line of readFileSync(envPath, 'utf8').split('\n')) {
    const t = line.trim();
    if (!t || t.startsWith('#')) continue;
    const eq = t.indexOf('=');
    if (eq < 1) continue;
    const k = t.slice(0, eq).trim();
    const v = t.slice(eq + 1).trim();
    if (k && !(k in process.env)) process.env[k] = v;
  }
}

const PORT          = parseInt(process.env.WEBHOOK_PORT   || '9456');
const SECRET        = process.env.WEBHOOK_SECRET          || '';
const REPO_DIR      = process.env.REPO_DIR                || join(__dirname, '..');
const DEPLOY_BRANCH = process.env.DEPLOY_BRANCH           || 'main';
const SERVER_DIR    = __dirname; // server/ 目录

if (!SECRET) {
  console.warn('[webhook] ⚠  WEBHOOK_SECRET 未设置，将跳过签名验证（不建议生产使用）');
}

// ── 签名验证 ──────────────────────────────────────────────────────────────
function verifySignature(body, signature) {
  if (!SECRET) return true; // 未设置 secret 时跳过
  if (!signature?.startsWith('sha256=')) return false;
  const expected = 'sha256=' + createHmac('sha256', SECRET).update(body).digest('hex');
  try {
    return timingSafeEqual(Buffer.from(signature), Buffer.from(expected));
  } catch {
    return false;
  }
}

// ── 执行部署 ──────────────────────────────────────────────────────────────
let deploying = false;

function deploy(pusher, ref) {
  if (deploying) {
    console.log('[webhook] 部署正在进行，跳过本次触发');
    return;
  }
  deploying = true;

  const timestamp = new Date().toISOString();
  console.log(`\n[webhook] 🚀 触发部署 | ${timestamp} | ${pusher} → ${ref}`);

  const cmd = [
    `cd ${REPO_DIR}`,
    'git pull origin main',
    `cd ${SERVER_DIR}`,
    'docker compose up -d --build 2>&1',
  ].join(' && ');

  exec(cmd, { timeout: 300_000 }, (err, stdout, stderr) => {
    deploying = false;
    if (err) {
      console.error('[webhook] ✗ 部署失败:', err.message);
      if (stderr) console.error('[webhook] stderr:', stderr.slice(0, 500));
    } else {
      console.log('[webhook] ✓ 部署成功');
      // 只打印最后几行避免日志爆炸
      const lines = stdout.trim().split('\n');
      lines.slice(-8).forEach(l => console.log('[webhook]  ', l));
    }
  });
}

// ── HTTP 服务器 ────────────────────────────────────────────────────────────
const server = createServer((req, res) => {
  // 健康检查
  if (req.method === 'GET' && req.url === '/health') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ ok: true, deploying, timestamp: new Date().toISOString() }));
    return;
  }

  // 只处理 POST /webhook
  if (req.method !== 'POST' || req.url !== '/webhook') {
    res.writeHead(404);
    res.end('Not found');
    return;
  }

  const chunks = [];
  req.on('data', c => chunks.push(c));
  req.on('end', () => {
    const body = Buffer.concat(chunks);
    const signature = req.headers['x-hub-signature-256'];

    // 验证签名
    if (!verifySignature(body, signature)) {
      console.warn('[webhook] ✗ 签名验证失败，忽略请求');
      res.writeHead(401);
      res.end('Unauthorized');
      return;
    }

    const event = req.headers['x-github-event'];

    // 只处理 push 事件
    if (event !== 'push') {
      res.writeHead(200);
      res.end('ignored');
      return;
    }

    let payload;
    try {
      payload = JSON.parse(body.toString('utf8'));
    } catch {
      res.writeHead(400);
      res.end('Bad JSON');
      return;
    }

    const ref    = payload.ref || '';
    const pusher = payload.pusher?.name || 'unknown';
    const branch = ref.replace('refs/heads/', '');

    // 只响应目标分支
    if (branch !== DEPLOY_BRANCH) {
      console.log(`[webhook] 忽略分支 ${branch}（目标：${DEPLOY_BRANCH}）`);
      res.writeHead(200);
      res.end('ignored branch');
      return;
    }

    res.writeHead(200);
    res.end('deploying');

    // 异步执行，不阻塞响应
    deploy(pusher, ref);
  });
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`[webhook] 📡 监听 http://127.0.0.1:${PORT}`);
  console.log(`[webhook] 📁 仓库目录: ${REPO_DIR}`);
  console.log(`[webhook] 🌿 触发分支: ${DEPLOY_BRANCH}`);
  console.log(`[webhook] 🔐 签名验证: ${SECRET ? '已启用' : '未启用'}`);
});
