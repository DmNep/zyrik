# Настоящие рабочие столы Linux (X11) для проверки Зырика: KDE Plasma, XFCE, GNOME (Ubuntu 24.04).
# Сборка:  docker build -t zyrik-desktops tools/linux-desktops
# KDE и XFCE:
#   docker run --rm -v <папка с zyrik>:/bin-zyrik:ro -v <папка для снимков>:/out zyrik-desktops kde|xfce
# GNOME нужен systemd-logind — контейнер с systemd вместо init:
#   docker run -d --name zyrik-gnome --privileged --cgroupns=host -v /sys/fs/cgroup:/sys/fs/cgroup:rw #     --tmpfs /run --tmpfs /run/lock -v <папка с zyrik>:/bin-zyrik:ro --user root --entrypoint /sbin/init zyrik-desktops
#   docker exec zyrik-gnome bash -c 'mkdir -p /out && chmod 777 /out'
#   docker exec -u tester zyrik-gnome bash /run.sh gnome; docker rm -f zyrik-gnome
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      xvfb xdotool xclip x11-utils x11-xserver-utils imagemagick dbus-x11 dbus libgl1-mesa-dri libglx-mesa0 \
      locales fonts-dejavu-core strace python3 \
      plasma-desktop plasma-workspace kwin-x11 qdbus-qt5 kde-cli-tools \
      xfce4-panel xfwm4 xfconf xfce4-settings \
      gnome-shell gnome-shell-extension-appindicator \
    && locale-gen ru_RU.UTF-8 && rm -rf /var/lib/apt/lists/*
RUN useradd -m tester
COPY run.sh /run.sh
USER tester
ENV LANG=ru_RU.UTF-8 XDG_RUNTIME_DIR=/tmp/xdg LIBGL_ALWAYS_SOFTWARE=1
ENTRYPOINT ["bash", "/run.sh"]
