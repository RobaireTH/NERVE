FROM node:20-alpine AS builder
WORKDIR /app

COPY packages/mcp/package*.json ./
RUN npm ci

COPY packages/mcp/tsconfig.json ./
COPY packages/mcp/src ./src
RUN npm run build

FROM node:20-alpine
WORKDIR /app
COPY --from=builder /app/dist ./dist
COPY packages/mcp/package*.json ./
RUN npm ci --omit=dev
EXPOSE 8081
CMD ["node", "dist/index.js"]
