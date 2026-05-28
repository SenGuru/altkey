FROM python:3.12-slim

# curl_cffi needs a recent libcurl-impersonate; the wheel bundles it, but
# certutil/ca-certificates keep TLS happy.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# App code (the proven local engine) is copied in at build time.
COPY app ./app

# HF Spaces route external traffic to port 7860.
ENV ALTKEY_HOST=0.0.0.0 \
    ALTKEY_PORT=7860 \
    ALTKEY_HOME=/data

# /data is writable on HF Spaces (persistent if a disk is attached, ephemeral
# otherwise — fine for a test).
RUN mkdir -p /data

EXPOSE 7860

CMD ["python", "-m", "app.main"]
