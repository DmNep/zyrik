# Свежие композиторы Wayland (Arch Linux): KDE Plasma 6 (KWin), GNOME (Mutter), Hyprland.
# Для проверки Зырика там, где нет wlroots: что из протоколов есть и как работают порталы.
FROM archlinux:latest
RUN pacman -Syu --noconfirm --needed \
      kwin plasma-workspace xdg-desktop-portal xdg-desktop-portal-kde \
      gnome-shell mutter xdg-desktop-portal-gnome \
      hyprland xdg-desktop-portal-hyprland \
      wayland-utils wl-clipboard wtype grim strace python dbus mesa ttf-dejavu \
    && pacman -Scc --noconfirm
# вложенный Hyprland запускается внутри Sway без экрана
RUN for i in 1 2 3; do pacman -S --noconfirm --needed sway && break; sleep 5; done && pacman -Scc --noconfirm
RUN useradd -m tester
USER tester
ENV XDG_RUNTIME_DIR=/tmp/xdg LANG=C.UTF-8
