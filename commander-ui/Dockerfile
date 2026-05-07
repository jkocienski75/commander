# syntax=docker/dockerfile:1.7

# ---- development stage ----
# For dev, we run the Vite dev server with hot reload
FROM node:22-alpine AS dev

WORKDIR /app

COPY package*.json ./
RUN npm ci

COPY . .

EXPOSE 8210

CMD ["npm", "run", "dev", "--", "--host", "0.0.0.0"]

# ---- build stage ----
FROM node:22-alpine AS build

WORKDIR /app

COPY package*.json ./
RUN npm ci

COPY . .
RUN npm run build

# ---- production stage ----
FROM nginx:alpine AS production

COPY --from=build /app/dist /usr/share/nginx/html
COPY nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 8210

CMD ["nginx", "-g", "daemon off;"]
