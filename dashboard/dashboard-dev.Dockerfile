# Development Dockerfile for Dashboard with hot reloading
FROM node:22-alpine

WORKDIR /app

# Install dependencies first for better caching
COPY package*.json ./
RUN npm i

# Copy source code
COPY . .

# Expose development port
EXPOSE 3000

# Set environment for development
ENV NODE_ENV=development

# Use npm run dev for hot reloading
CMD ["npm", "run", "dev"]
