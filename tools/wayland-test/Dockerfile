# Зырик под Wayland в контейнере: Sway без экрана (headless), wtype и своя виртуальная мышь вместо рук.
# Сборка:  docker build -t zyrik-wayland tools/wayland-test
# Запуск:  docker run --rm -v <папка с zyrik и wl_mouse>:/bin-zyrik:ro -v <папка для снимков>:/out zyrik-wayland sway
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      sway wtype wl-clipboard grim strace python3 dbus-x11 dbus locales fonts-dejavu-core \
    && locale-gen ru_RU.UTF-8 && rm -rf /var/lib/apt/lists/*
RUN useradd -m tester
COPY run.sh /run.sh
USER tester
ENV LANG=ru_RU.UTF-8 XDG_RUNTIME_DIR=/tmp/xdg
ENTRYPOINT ["bash", "/run.sh"]
