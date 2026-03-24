# Stage 1: Build
FROM node:24-alpine AS builder
RUN corepack enable
WORKDIR /app
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml turbo.json ./
COPY apps/ apps/
COPY packages/ packages/
RUN pnpm install --frozen-lockfile
RUN pnpm turbo build --filter=@emailibrium/web

# Stage 2: Runtime
FROM nginx:alpine AS runtime
COPY --from=builder /app/apps/web/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf
USER 1000:1000

# Stage 3: Development
FROM node:24-alpine AS development
RUN corepack enable
WORKDIR /app
CMD ["pnpm", "dev"]
