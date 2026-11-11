# ---- build stage: install deps into a venv -----------------------------------
FROM python:3.12-alpine AS build
ENV PIP_NO_CACHE_DIR=1 PIP_DISABLE_PIP_VERSION_CHECK=1
COPY requirements.txt /tmp/
RUN python -m venv /venv \
 && /venv/bin/pip install -r /tmp/requirements.txt \
 && /venv/bin/pip uninstall -y pip setuptools \
 && find /venv -type d -name __pycache__ -prune -exec rm -rf {} +

# ---- runtime stage -----------------------------------------------------------
FROM python:3.12-alpine
RUN adduser -S -D -H -u 10001 app
COPY --from=build /venv /venv
COPY app /srv/app
WORKDIR /srv
ENV PATH=/venv/bin:$PATH PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
USER app
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s CMD wget -qO- http://127.0.0.1:8000/healthz >/dev/null || exit 1
CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000", "--no-server-header"]
