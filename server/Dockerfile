FROM node:22-alpine

WORKDIR /app

# 只复制依赖描述，利用 layer 缓存
COPY package.json package-lock.json ./

RUN npm ci --omit=dev

# 复制源码
COPY src ./src

EXPOSE 3456

CMD ["node", "src/app.js"]
