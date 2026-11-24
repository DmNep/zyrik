# Проверка Зырика под Linux X11 в контейнере: Xvfb вместо экрана, xdotool вместо рук.
# Сборка: docker build -t zyrik-x11 tools/linux-test
# Запуск: docker run --rm -v "$PWD:/src:ro" zyrik-x11
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends xvfb xdotool xclip strace x11-utils x11-xkb-utils dbus-x11 python3 cmake ninja-build && rm -rf /var/lib/apt/lists/*
RUN useradd -m tester && mkdir -p /home/tester/cargo /home/tester/target && chown -R tester /home/tester
COPY run.sh /run.sh
USER tester
ENV LANG=C.UTF-8 CARGO_TARGET_DIR=/home/tester/target CARGO_HOME=/home/tester/cargo
CMD ["bash", "/run.sh"]
