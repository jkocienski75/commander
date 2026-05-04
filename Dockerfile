# syntax=docker/dockerfile:1.7

# ---- build stage ----
FROM eclipse-temurin:21-jdk-jammy AS build

WORKDIR /workspace

# POMs and Maven wrapper first so dependency resolution layer caches independently of source
COPY .mvn .mvn
COPY mvnw mvnw
COPY pom.xml pom.xml
COPY commander-app/pom.xml commander-app/pom.xml

RUN chmod +x mvnw && ./mvnw -pl commander-app -am dependency:go-offline -B

# Copy everything else (.dockerignore filters target/, .git/, IDE files)
COPY . .

RUN ./mvnw -pl commander-app -am clean package -DskipTests -B

# ---- runtime stage ----
FROM eclipse-temurin:21-jre-jammy AS runtime

# curl is required for HEALTHCHECK and useful for operator debugging inside the container
RUN apt-get update \
    && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --uid 10001 --shell /usr/sbin/nologin commander

WORKDIR /app

COPY --from=build --chown=commander:commander \
     /workspace/commander-app/target/commander-app-*.jar \
     /app/commander-app.jar

USER commander

# 8200 = Commander backend HTTP, 8202 = actuator.
# JDWP (8201) is dev-only and enabled via JAVA_TOOL_OPTIONS in docker-compose.dev.yml.
EXPOSE 8200 8202

HEALTHCHECK --interval=10s --timeout=3s --start-period=30s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8202/actuator/health || exit 1

ENTRYPOINT ["java", "-jar", "/app/commander-app.jar"]
