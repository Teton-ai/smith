FROM postgres:15

ARG PARTMAN_VERSION="v5.1.0"

# Fix for GPG signature verification issues
RUN apt-get update -y \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        gnupg \
        dirmngr \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Add trusted keys
RUN mkdir -p /etc/apt/keyrings \
    && apt-get update -y \
    && apt-get install -y --no-install-recommends wget \
    && wget -qO- https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor > /etc/apt/keyrings/postgresql.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/postgresql.gpg] http://apt.postgresql.org/pub/repos/apt/ $(. /etc/os-release && echo $VERSION_CODENAME)-pgdg main" > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update -y

# Install required packages
RUN apt-get install -y --no-install-recommends \
    wget \
    gcc \
    make \
    build-essential \
    postgresql-server-dev-15 \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Install pg_partman
RUN cd /tmp \
    && wget "https://github.com/pgpartman/pg_partman/archive/refs/tags/${PARTMAN_VERSION}.tar.gz" \
    && tar zxf ${PARTMAN_VERSION}.tar.gz && cd pg_partman-${PARTMAN_VERSION#v} \
    && make \
    && make install \
    && cd .. && rm -r pg_partman-${PARTMAN_VERSION#v} ${PARTMAN_VERSION}.tar.gz

# Copy initialization script
COPY scripts/init-db.sh /docker-entrypoint-initdb.d/
RUN chmod +x /docker-entrypoint-initdb.d/init-db.sh
