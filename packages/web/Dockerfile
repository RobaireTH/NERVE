FROM node:20-alpine AS builder
WORKDIR /app

COPY packages/web/package*.json ./
RUN npm install

COPY packages/web/tsconfig.json ./
COPY packages/web/tsconfig.node.json ./
COPY packages/web/vite.config.ts ./
COPY packages/web/index.html ./
COPY packages/web/src ./src
RUN npm run build

FROM nginx:1.27-alpine
COPY packages/web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 80
