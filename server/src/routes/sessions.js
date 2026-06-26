import { query } from '../db/client.js';
import { writeFile, readFile, mkdir } from 'fs/promises';
import { existsSync } from 'fs';
import { join, dirname } from 'path';
import auth from '../middleware/auth.js';

const STORAGE_DIR = process.env.STORAGE_DIR || './storage';

// 确保存储目录存在
async function ensureStorageDir(subDir) {
  const dir = join(STORAGE_DIR, subDir);
  if (!existsSync(dir)) {
    await mkdir(dir, { recursive: true });
  }
  return dir;
}

export default async function sessionRoutes(fastify) {

  // ─── 上传/更新单条会话（含消息内容） ───────────────────────────────────────
  // POST /api/sessions/upload
  // Body: { session: {...}, messages: [{role, content, timestamp}...] }
  fastify.post('/sessions/upload', { preHandler: auth }, async (req, reply) => {
    const { session, messages } = req.body;

    if (!session?.id || !session?.device_id) {
      return reply.code(400).send({ error: 'Missing session.id or session.device_id' });
    }

    // 注册/更新设备信息
    await query(
      `INSERT INTO devices (id, name, platform, last_seen)
       VALUES ($1, $2, $3, now())
       ON CONFLICT (id) DO UPDATE SET name = $2, platform = $3, last_seen = now()`,
      [session.device_id, session.device_name || '', session.platform || '']
    );

    // 存储消息 JSONL 文件
    let messagesPath = null;
    if (messages && messages.length > 0) {
      const subDir = session.device_id;
      await ensureStorageDir(subDir);
      messagesPath = join(subDir, `${session.id}.jsonl`);
      const fullPath = join(STORAGE_DIR, messagesPath);
      const jsonlContent = messages.map(m => JSON.stringify(m)).join('\n');
      await writeFile(fullPath, jsonlContent, 'utf8');
    }

    // Upsert 会话元数据（以 updated_at_ms 为准，更新的覆盖旧的）
    const result = await query(
      `INSERT INTO sessions (
        id, device_id, device_name, title, cwd, model, model_provider,
        first_user_message, preview, created_at_ms, updated_at_ms,
        archived, messages_path, synced_at, message_count
       ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,now(),$14)
       ON CONFLICT (id) DO UPDATE SET
         device_id      = EXCLUDED.device_id,
         device_name    = EXCLUDED.device_name,
         title          = EXCLUDED.title,
         cwd            = EXCLUDED.cwd,
         model          = EXCLUDED.model,
         model_provider = EXCLUDED.model_provider,
         first_user_message = EXCLUDED.first_user_message,
         preview        = EXCLUDED.preview,
         updated_at_ms  = EXCLUDED.updated_at_ms,
         archived       = EXCLUDED.archived,
         messages_path  = COALESCE(EXCLUDED.messages_path, sessions.messages_path),
         synced_at      = now(),
         message_count  = EXCLUDED.message_count
       WHERE EXCLUDED.updated_at_ms >= sessions.updated_at_ms
       RETURNING id`,
      [
        session.id,
        session.device_id,
        session.device_name || '',
        session.title || '',
        session.cwd || '',
        session.model || '',
        session.model_provider || '',
        session.first_user_message || '',
        session.preview || '',
        session.created_at_ms || 0,
        session.updated_at_ms || 0,
        session.archived || false,
        messagesPath,
        messages?.length || 0,
      ]
    );

    // 写入同步日志
    await query(
      `INSERT INTO sync_log (device_id, session_id, action, status, detail)
       VALUES ($1, $2, 'upload', 'ok', $3)`,
      [session.device_id, session.id, `message_count=${messages?.length || 0}`]
    );

    const updated = result.rows.length > 0;
    return { ok: true, id: session.id, updated };
  });


  // ─── 批量上传（增量同步核心接口） ─────────────────────────────────────────
  // POST /api/sessions/upload-batch
  // Body: { sessions: [{session, messages}...] }
  fastify.post('/sessions/upload-batch', { preHandler: auth }, async (req, reply) => {
    const { sessions } = req.body;

    if (!Array.isArray(sessions) || sessions.length === 0) {
      return reply.code(400).send({ error: 'sessions must be a non-empty array' });
    }
    if (sessions.length > 200) {
      return reply.code(400).send({ error: 'Max 200 sessions per batch' });
    }

    const results = [];
    for (const item of sessions) {
      try {
        // 复用单条上传逻辑
        const mockReq = { body: item };
        const mockReply = {
          code: () => mockReply,
          send: (v) => { results.push({ id: item.session?.id, ok: false, error: v?.error }); return mockReply; },
        };

        // 注册/更新设备
        const { session, messages } = item;
        if (!session?.id || !session?.device_id) {
          results.push({ id: session?.id, ok: false, error: 'Missing id or device_id' });
          continue;
        }

        await query(
          `INSERT INTO devices (id, name, platform, last_seen)
           VALUES ($1, $2, $3, now())
           ON CONFLICT (id) DO UPDATE SET name = $2, platform = $3, last_seen = now()`,
          [session.device_id, session.device_name || '', session.platform || '']
        );

        let messagesPath = null;
        if (messages && messages.length > 0) {
          const subDir = session.device_id;
          await ensureStorageDir(subDir);
          messagesPath = join(subDir, `${session.id}.jsonl`);
          const fullPath = join(STORAGE_DIR, messagesPath);
          const jsonlContent = messages.map(m => JSON.stringify(m)).join('\n');
          await writeFile(fullPath, jsonlContent, 'utf8');
        }

        const res = await query(
          `INSERT INTO sessions (
            id, device_id, device_name, title, cwd, model, model_provider,
            first_user_message, preview, created_at_ms, updated_at_ms,
            archived, messages_path, synced_at, message_count
           ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,now(),$14)
           ON CONFLICT (id) DO UPDATE SET
             device_id = EXCLUDED.device_id,
             device_name = EXCLUDED.device_name,
             title = EXCLUDED.title, cwd = EXCLUDED.cwd,
             model = EXCLUDED.model, model_provider = EXCLUDED.model_provider,
             first_user_message = EXCLUDED.first_user_message,
             preview = EXCLUDED.preview,
             updated_at_ms = EXCLUDED.updated_at_ms,
             archived = EXCLUDED.archived,
             messages_path = COALESCE(EXCLUDED.messages_path, sessions.messages_path),
             synced_at = now(),
             message_count = EXCLUDED.message_count
           WHERE EXCLUDED.updated_at_ms >= sessions.updated_at_ms
           RETURNING id`,
          [
            session.id, session.device_id, session.device_name || '',
            session.title || '', session.cwd || '', session.model || '',
            session.model_provider || '', session.first_user_message || '',
            session.preview || '', session.created_at_ms || 0,
            session.updated_at_ms || 0, session.archived || false,
            messagesPath, messages?.length || 0,
          ]
        );

        results.push({ id: session.id, ok: true, updated: res.rows.length > 0 });
      } catch (err) {
        results.push({ id: item.session?.id, ok: false, error: err.message });
      }
    }

    // 批量写同步日志
    const okIds = results.filter(r => r.ok).map(r => r.id);
    if (okIds.length > 0) {
      const deviceId = sessions[0]?.session?.device_id;
      for (const id of okIds) {
        await query(
          `INSERT INTO sync_log (device_id, session_id, action, status)
           VALUES ($1, $2, 'upload', 'ok')`,
          [deviceId, id]
        );
      }
    }

    return {
      ok: true,
      total: sessions.length,
      uploaded: results.filter(r => r.ok).length,
      failed: results.filter(r => !r.ok).length,
      results,
    };
  });


  // ─── 获取会话列表（支持分页 + 设备过滤） ─────────────────────────────────
  // GET /api/sessions?page=0&pageSize=30&device_id=xxx
  fastify.get('/sessions', { preHandler: auth }, async (req, reply) => {
    const page = parseInt(req.query.page || '0');
    const pageSize = Math.min(parseInt(req.query.pageSize || '30'), 100);
    const deviceId = req.query.device_id || null;
    const offset = page * pageSize;

    let sql = `SELECT id, device_id, device_name, title, cwd, model,
                      first_user_message, preview, created_at_ms, updated_at_ms,
                      archived, message_count, synced_at
               FROM sessions`;
    const params = [];

    if (deviceId) {
      sql += ` WHERE device_id = $${params.length + 1}`;
      params.push(deviceId);
    }

    sql += ` ORDER BY updated_at_ms DESC LIMIT $${params.length + 1} OFFSET $${params.length + 2}`;
    params.push(pageSize, offset);

    const result = await query(sql, params);

    // 总数
    let countSql = 'SELECT COUNT(*) FROM sessions';
    const countParams = [];
    if (deviceId) {
      countSql += ' WHERE device_id = $1';
      countParams.push(deviceId);
    }
    const countResult = await query(countSql, countParams);

    return {
      sessions: result.rows,
      total: parseInt(countResult.rows[0].count),
      page,
      pageSize,
    };
  });


  // ─── 获取单条会话的消息内容 ───────────────────────────────────────────────
  // GET /api/sessions/:id/messages
  fastify.get('/sessions/:id/messages', { preHandler: auth }, async (req, reply) => {
    const { id } = req.params;

    const result = await query(
      'SELECT messages_path FROM sessions WHERE id = $1',
      [id]
    );

    if (result.rows.length === 0) {
      return reply.code(404).send({ error: 'Session not found' });
    }

    const { messages_path } = result.rows[0];
    if (!messages_path) {
      return { messages: [] };
    }

    const fullPath = join(STORAGE_DIR, messages_path);
    if (!existsSync(fullPath)) {
      return { messages: [] };
    }

    const content = await readFile(fullPath, 'utf8');
    const messages = content
      .split('\n')
      .filter(line => line.trim())
      .map(line => {
        try { return JSON.parse(line); } catch { return null; }
      })
      .filter(Boolean);

    return { messages };
  });


  // ─── 搜索会话 ────────────────────────────────────────────────────────────
  // GET /api/sessions/search?q=keyword&device_id=xxx
  fastify.get('/sessions/search', { preHandler: auth }, async (req, reply) => {
    const q = (req.query.q || '').trim();
    const deviceId = req.query.device_id || null;

    if (!q) {
      return reply.code(400).send({ error: 'Missing query parameter q' });
    }

    const likeQ = `%${q}%`;
    const params = [likeQ, likeQ, likeQ, likeQ];
    let sql = `SELECT id, device_id, device_name, title, cwd, model,
                      first_user_message, preview, created_at_ms, updated_at_ms,
                      archived, message_count
               FROM sessions
               WHERE (LOWER(title) LIKE LOWER($1)
                  OR LOWER(first_user_message) LIKE LOWER($2)
                  OR LOWER(preview) LIKE LOWER($3)
                  OR LOWER(cwd) LIKE LOWER($4))`;

    if (deviceId) {
      params.push(deviceId);
      sql += ` AND device_id = $${params.length}`;
    }

    sql += ' ORDER BY updated_at_ms DESC LIMIT 100';

    const result = await query(sql, params);
    return { sessions: result.rows, total: result.rows.length };
  });


  // ─── 删除会话 ────────────────────────────────────────────────────────────
  // DELETE /api/sessions/:id
  fastify.delete('/sessions/:id', { preHandler: auth }, async (req, reply) => {
    const { id } = req.params;
    const result = await query(
      'DELETE FROM sessions WHERE id = $1 RETURNING id, messages_path',
      [id]
    );
    if (result.rows.length === 0) {
      return reply.code(404).send({ error: 'Session not found' });
    }
    return { ok: true, id };
  });


  // ─── 获取服务端会话 ID 列表（用于客户端增量对比） ─────────────────────────
  // GET /api/sessions/ids?since=updated_at_ms&device_id=xxx
  fastify.get('/sessions/ids', { preHandler: auth }, async (req, reply) => {
    const since = parseInt(req.query.since || '0');
    const deviceId = req.query.device_id || null;

    let sql = `SELECT id, updated_at_ms FROM sessions WHERE updated_at_ms > $1`;
    const params = [since];

    if (deviceId) {
      params.push(deviceId);
      sql += ` AND device_id = $${params.length}`;
    }

    sql += ' ORDER BY updated_at_ms DESC';
    const result = await query(sql, params);

    return {
      ids: result.rows.map(r => ({ id: r.id, updated_at_ms: r.updated_at_ms }))
    };
  });

}
