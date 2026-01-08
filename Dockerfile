# --- build stage ---
FROM rust:latest as builder
WORKDIR /app
COPY . .
RUN cargo build --release

# --- runtime stage ---
FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the binary
COPY --from=builder /app/target/release/hybrid-connection-health /usr/local/bin/hybrid-connection-health

# Create an alias so 'agent' works as expected in commands
RUN ln -s /usr/local/bin/hybrid-connection-health /usr/local/bin/agent

CMD ["agent"]
