// Bearer Token 鉴权中间件
// 客户端请求时需在 Header 中携带: Authorization: Bearer <API_TOKEN>

export default async function authMiddleware(request, reply) {
  const authHeader = request.headers['authorization'];

  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    return reply.code(401).send({ error: 'Missing or invalid Authorization header' });
  }

  const token = authHeader.slice(7).trim();
  const expected = process.env.API_TOKEN;

  if (!expected || token !== expected) {
    return reply.code(403).send({ error: 'Invalid API token' });
  }
  // 通过鉴权，继续处理请求
}
