# Генератор корпуса снимков для проверки поиска опасного (T059): настоящие xterm и Chromium.
#   docker build -t zyrik-corpus tools/corpus
#   docker run --rm -v "$PWD:/src" zyrik-corpus   → tests/redact_corpus/*.png + *.json
FROM debian:bookworm
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      chromium xterm xvfb x11-apps imagemagick python3 python3-pil \
      fonts-dejavu-core fonts-liberation fonts-noto-core fonts-noto-mono locales \
    && sed -i 's/# ru_RU.UTF-8/ru_RU.UTF-8/; s/# en_US.UTF-8/en_US.UTF-8/' /etc/locale.gen && locale-gen \
    && rm -rf /var/lib/apt/lists/*
ENV LANG=ru_RU.UTF-8
CMD ["python3", "/src/tools/corpus/gen.py"]
