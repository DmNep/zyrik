#!/usr/bin/env python3
"""Корпус снимков для проверки поиска опасного (T059): настоящие xterm и Chromium в контейнере.

Все данные вымышленные и создаются случайно при каждом запуске (семя фиксировано — корпус
воспроизводим). Каждый секрет рисуется своим цветом, на единицу отличающимся от обычного текста:
на глаз и для распознавания разницы нет, а по точному цвету пикселей вычисляются рамки секрета.
В эталон (`*.json`) записываются только категория и рамки — сами значения в репозиторий не
попадают (это картинки).

Запуск: см. Dockerfile рядом. Результат: tests/redact_corpus/NN-имя.png и NN-имя.json.
"""
import hashlib
import html
import json
import os
import random
import string
import subprocess
import time

from PIL import Image

SRC = "/src"
OUT = os.path.join(SRC, "tests", "redact_corpus")
R = random.Random(20261122)
BIP39 = [w.strip() for w in open(os.path.join(SRC, "assets", "bip39-english.txt")) if w.strip()]

# ---------- вымышленные значения ----------

ALNUM = string.ascii_letters + string.digits
B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def rs(n, alphabet=ALNUM):
    return "".join(R.choice(alphabet) for _ in range(n))


def luhn_complete(prefix, length=16):
    d = [int(c) for c in prefix] + [R.randrange(10) for _ in range(length - len(prefix) - 1)]
    s = 0
    for i, x in enumerate(reversed(d)):
        if i % 2 == 0:
            x *= 2
            x -= 9 if x > 9 else 0
        s += x
    return "".join(map(str, d)) + str((10 - s % 10) % 10)


def card():
    n = luhn_complete(R.choice(["4", "51", "2200", "2202"]))
    return " ".join(n[i:i + 4] for i in range(0, 16, 4))


def snils():
    while True:
        d = [R.randrange(10) for _ in range(9)]
        s = sum(x * w for x, w in zip(d, range(9, 0, -1)))
        c = s if s < 100 else 0 if s in (100, 101) else (s % 101) % 100
        t = "".join(map(str, d))
        return f"{t[:3]}-{t[3:6]}-{t[6:9]} {c:02d}"


def inn(n=12):
    def dig(d, w):
        return sum(x * y for x, y in zip(d, w)) % 11 % 10
    d = [R.randrange(1, 10)] + [R.randrange(10) for _ in range(n - 3 if n == 12 else n - 2)]
    if n == 10:
        d.append(dig(d, [2, 4, 10, 3, 5, 9, 4, 6, 8]))
    else:
        d.append(dig(d, [7, 2, 4, 10, 3, 5, 9, 4, 6, 8]))
        d.append(dig(d, [3, 7, 2, 4, 10, 3, 5, 9, 4, 6, 8]))
    return "".join(map(str, d))


def b58check(payload):
    data = payload + hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    n = int.from_bytes(data, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = B58[r] + out
    return "1" * (len(data) - len(data.lstrip(b"\0"))) + out


def btc_legacy():
    return b58check(bytes([R.choice([0, 5])]) + R.randbytes(20))


def wif():
    return b58check(b"\x80" + R.randbytes(32) + b"\x01")


def bech32():
    cs = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    gen = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]

    def polymod(v):
        c = 1
        for x in v:
            top = c >> 25
            c = (c & 0x1ffffff) << 5 ^ x
            for i in range(5):
                c ^= gen[i] if (top >> i) & 1 else 0
        return c
    hrp = "bc"
    data = [0] + [R.randrange(32) for _ in range(32)]
    values = [ord(x) >> 5 for x in hrp] + [0] + [ord(x) & 31 for x in hrp] + data
    pm = polymod(values + [0] * 6) ^ 1
    chk = [(pm >> 5 * (5 - i)) & 31 for i in range(6)]
    return hrp + "1" + "".join(cs[x] for x in data + chk)


def b64(n):
    return rs(n, string.ascii_letters + string.digits + "+/")


def b64url(raw):
    import base64
    return base64.urlsafe_b64encode(raw.encode()).decode().rstrip("=")


def jwt():
    return b64url('{"alg":"HS256","typ":"JWT"}') + "." + b64url('{"sub":"%d","name":"Test User","iat":%d}' % (R.randrange(10**6), 1700000000 + R.randrange(10**7))) + "." + rs(43, ALNUM + "-_")


NAMES = [("Петров", "Иван", "Сергеевич"), ("Смирнова", "Анна", "Викторовна"), ("Кузнецов", "Дмитрий", "Олегович"),
         ("Соколова", "Мария", "Андреевна"), ("Волков", "Алексей", "Игоревич"), ("Морозова", "Елена", "Павловна")]
LOGINS = ["ivanov", "petrov", "smirnova", "kuznetsov", "admin", "deploy", "sokolova", "volkov"]


def fio():
    f, i, o = R.choice(NAMES)
    return f"{f} {i} {o}"


def fio_short():
    f, i, o = R.choice(NAMES)
    return f"{f} {i[0]}.{o[0]}."


def email():
    return f"{R.choice(LOGINS)}.{rs(3, string.ascii_lowercase)}@{R.choice(['example.com', 'example.org', 'mail.example.ru', 'corp.example.net'])}"


def phone():
    return R.choice([f"+7 (9{R.randrange(10, 99)}) {R.randrange(100, 999)}-{R.randrange(10, 99)}-{R.randrange(10, 99)}",
                     f"8 9{R.randrange(10, 99)} {R.randrange(100, 999)} {R.randrange(10, 99)} {R.randrange(10, 99)}",
                     f"+1 (415) 555-{R.randrange(1000, 9999)}"])


def ipv4():
    return R.choice([f"10.{R.randrange(256)}.{R.randrange(256)}.{R.randrange(1, 255)}", f"192.168.{R.randrange(256)}.{R.randrange(1, 255)}",
                     f"172.{R.randrange(16, 32)}.{R.randrange(256)}.{R.randrange(1, 255)}", f"{R.randrange(11, 223)}.{R.randrange(256)}.{R.randrange(256)}.{R.randrange(1, 255)}"])


def ipv6():
    return "fe80::" + ":".join("%x" % R.randrange(0x100, 0xffff) for _ in range(4))


def mac():
    return ":".join("%02x" % R.randrange(256) for _ in range(6))


def host():
    return f"{R.choice(['nas', 'db01', 'git', 'jenkins', 'srv-1c', 'mail'])}.{R.choice(['corp', 'local', 'lan', 'internal'])}"


def seed(n=12):
    return R.sample(BIP39, n)


def pem_lines():
    lines = ["-----BEGIN OPENSSH PRIVATE KEY-----"] + [b64(70) for _ in range(5)] + [b64(36) + "=", "-----END OPENSSH PRIVATE KEY-----"]
    return lines


# ---------- сцены ----------
# Строка сцены — список кусков: обычный текст (str) или секрет (категория, текст).
# Многострочный секрет — один и тот же объект Secret в нескольких строках.

class Secret:
    def __init__(self, cat, text):
        self.cat, self.text = cat, text


def S(cat, text):
    return Secret(cat, text)


def term_scenes():
    sc = []
    u = R.choice(LOGINS)
    sc.append(("term-env", [
        [f"{u}@laptop:~$ env | grep -i -E 'key|token|pass'"],
        ["AWS_ACCESS_KEY_ID=", S("Token", "AKIA" + rs(16, string.ascii_uppercase + string.digits))],
        ["GITHUB_TOKEN=", S("Token", "ghp_" + rs(36))],
        ["DB_PASSWORD=", S("Password", rs(14))],
        ["SLACK_BOT_TOKEN=", S("Token", "xoxb-" + rs(12, string.digits) + "-" + rs(13, string.digits) + "-" + rs(24))],
        [f"{u}@laptop:~$ "],
    ]))
    sc.append(("term-git", [
        [f"{u}@build:~/src$ git clone https://deploy:", S("UrlCredentials", rs(20)), f"@git.corp/app.git"],
        ["Cloning into 'app'..."],
        ["remote: Enumerating objects: 1532, done."],
        ["Receiving objects: 100% (1532/1532), 2.41 MiB | 11.2 MiB/s, done."],
        [f"{u}@build:~/src$ "],
    ]))
    sc.append(("term-ssh", [
        [f"{u}@laptop:~$ ssh root@", S("Ip", ipv4()), " -i ", S("UserPath", f"/home/{u}"), "/.ssh/id_ed25519"],
        ["Welcome to Ubuntu 24.04 LTS (GNU/Linux 6.8.0-45-generic x86_64)"],
        ["Last login: Fri Nov 21 18:02:11 2026 from ", S("Ip", ipv4())],
        ["root@prod-db:~# "],
    ]))
    # блок ключа — один секрет на всех строках
    blk = S("PrivateKey", "")
    sc.append(("term-pem", [[f"{u}@laptop:~$ cat ~/.ssh/id_ed25519"]] + [[(blk, l)] for l in pem_lines()] + [[f"{u}@laptop:~$ "]]))
    sc.append(("term-sshpub", [
        [f"{u}@laptop:~$ cat ~/.ssh/id_ed25519.pub"],
        [S("SshKey", "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI" + b64(43)), f" {u}@laptop"],
        [f"{u}@laptop:~$ "],
    ]))
    sc.append(("term-ifconfig", [
        [f"{u}@laptop:~$ ip addr show eth0"],
        ["2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP"],
        ["    link/ether ", S("Mac", mac()), " brd ff:ff:ff:ff:ff:ff"],
        ["    inet ", S("Ip", ipv4()), "/24 brd ", S("Ip", "10.0.0.255"), " scope global eth0"],
        ["    inet6 ", S("Ip", ipv6()), "/64 scope link"],
    ]))
    sc.append(("term-psql", [
        [f"{u}@laptop:~$ psql ", S("UrlCredentials", f"postgres://app:{rs(12)}@{host()}:5432/billing")],
        ["psql (16.4)"],
        ["billing=> select count(*) from invoices;"],
        ["  count"],
        ["--------"],
        ["  48211"],
    ]))
    sd = S("SeedPhrase", "")
    sc.append(("term-wallet", [
        [f"{u}@laptop:~$ wallet-cli create --words 12"],
        ["Write down your recovery phrase and keep it safe:"],
        [(sd, " ".join(seed(12)))],
        ["Address: ", S("Wallet", bech32())],
    ]))
    sc.append(("term-curl", [
        [f"{u}@laptop:~$ curl -H \"Authorization: Bearer ", S("Token", rs(40, ALNUM + "._-")), "\" https://api.example.com/v1/me"],
        ['{"id": 4182, "email": "', S("Email", email()), '", "plan": "pro"}'],
    ]))
    sc.append(("term-docker", [
        [f"{u}@ci:~$ docker login -u robot -p ", S("Password", rs(18)), " registry.example.com"],
        ["WARNING! Using --password via the CLI is insecure. Use --password-stdin."],
        ["Login Succeeded"],
    ]))
    # чистые
    sc.append(("term-clean-cargo", [
        [f"{u}@laptop:~/zyrik$ cargo build --release"],
        ["   Compiling libc v0.2.159"],
        ["   Compiling serde v1.0.210"],
        ["   Compiling regex-syntax v0.8.5"],
        ["   Compiling zyrik v0.1.0 (/src)"],
        ["    Finished `release` profile [optimized] target(s) in 41.27s"],
    ]))
    sc.append(("term-clean-ls", [
        [f"{u}@laptop:~$ ls -la"],
        ["drwxr-xr-x 12 user user  4096 Nov 21 18:02 ."],
        ["-rw-r--r--  1 user user   220 Mar 31  2024 .bash_logout"],
        ["-rw-r--r--  1 user user  3771 Mar 31  2024 .bashrc"],
        ["drwxr-xr-x  3 user user  4096 Oct  2 11:40 Documents"],
        ["-rw-r--r--  1 user user 81920 Nov 20 09:15 report-2026.ods"],
    ]))
    sc.append(("term-clean-top", [
        ["top - 18:04:12 up 3 days,  2:11,  1 user,  load average: 0.42, 0.37, 0.31"],
        ["Tasks: 312 total,   1 running, 311 sleeping,   0 stopped,   0 zombie"],
        ["%Cpu(s):  3.1 us,  1.2 sy,  0.0 ni, 95.4 id,  0.2 wa,  0.0 hi,  0.1 si"],
        ["MiB Mem :  31842.6 total,  18210.4 free,   6143.0 used,   7489.2 buff/cache"],
        ["    PID USER      PR  NI    VIRT    RES    SHR S  %CPU  %MEM     TIME+ COMMAND"],
        ["   2141 user      20   0 4211244 412880 141056 S   4.3   1.3  12:41.07 firefox"],
    ]))
    return sc


THEMES = [
    ("#1e1e1e", "#d4d4d4"),  # тёмная
    ("#ffffff", "#1e1e1e"),  # светлая
    ("#002b36", "#c8d0d0"),  # solarized-подобная
]
FONTS = [("DejaVu Sans Mono", 13), ("Liberation Mono", 14), ("Noto Mono", 12), ("DejaVu Sans Mono", 16), ("Liberation Mono", 11)]


def secret_color(fg, i):
    """Цвет секрета i: как обычный текст, но синий канал сдвинут на i+1 (в сторону фона не важно)."""
    r, g, b = (int(fg[k:k + 2], 16) for k in (1, 3, 5))
    b = b + i + 1 if b + i + 1 <= 255 else b - i - 1
    return "#%02x%02x%02x" % (r, g, b)


def assign_colors(rows, fg):
    """Секреты сцены → цвета. Возвращает [(секрет, цвет)] и функцию цвета куска."""
    seen = []
    for row in rows:
        for part in row:
            sec = part[0] if isinstance(part, tuple) else part if isinstance(part, Secret) else None
            if sec is not None and all(sec is not s for s in seen):
                seen.append(sec)
    return [(s, secret_color(fg, i)) for i, s in enumerate(seen)]


def parts(row):
    """Кусок строки → (текст, секрет или None)."""
    for p in row:
        if isinstance(p, tuple):
            yield p[1], p[0]
        elif isinstance(p, Secret):
            yield p.text, p
        else:
            yield p, None


def boxes(png, palette):
    """Рамки по точному цвету: для каждого секрета — по строкам пикселей, сгруппированным в строки текста."""
    img = Image.open(png).convert("RGB")
    w, h = img.size
    palette_rgb = [tuple(int(c[k:k + 2], 16) for k in (1, 3, 5)) for _, c in palette]
    want = {c: i for i, c in enumerate(palette_rgb)}
    rows = [dict() for _ in palette]  # y -> [xmin, xmax]
    px = img.load()
    for y in range(h):
        for x in range(w):
            i = want.get(px[x, y])
            if i is not None:
                r = rows[i].setdefault(y, [x, x])
                r[0] = min(r[0], x)
                r[1] = max(r[1], x)
    out = []
    for i, (sec, _) in enumerate(palette):
        ys = sorted(rows[i])
        rects = []
        start = prev = None
        for y in ys + [None]:
            if y is not None and prev is not None and y - prev <= 3:
                prev = y
                continue
            if start is not None:
                # по колонкам: столбцы пикселей с цветом секрета, разрыв больше двух высот строки — новая рамка
                cols = sorted({x for yy in range(start, prev + 1) for x in range(w) if px[x, yy] == palette_rgb[i]})
                gap = 2 * (prev - start + 1)
                seg = [cols[0], cols[0]]
                for x in cols[1:] + [None]:
                    if x is not None and x - seg[1] <= gap:
                        seg[1] = x
                        continue
                    rects.append([seg[0], start, seg[1] - seg[0] + 1, prev - start + 1])
                    if x is not None:
                        seg = [x, x]
            start = prev = y
        out.append({"category": sec.cat, "rects": rects})
    return out


def shoot_term(name, rows, theme, font):
    bg, fg = theme
    palette = assign_colors(rows, fg)
    color = {id(s): c for s, c in palette}

    def sgr(c):
        r, g, b = (int(c[k:k + 2], 16) for k in (1, 3, 5))
        return f"\x1b[38;2;{r};{g};{b}m"
    text = ""
    for row in rows:
        for t, sec in parts(row):
            text += (sgr(color[id(sec)]) + t + "\x1b[0m") if sec else t
        text += "\n"
    path = f"/tmp/{name}.txt"
    open(path, "w").write(text)
    cols = max(sum(len(t) for t, _ in parts(r)) for r in rows) + 2
    fam, size = font
    xt = subprocess.Popen(["xterm", "-geometry", f"{cols}x{len(rows) + 2}+0+0", "-fa", fam, "-fs", str(size),
                           "-bg", bg, "-fg", fg, "-xrm", "XTerm.vt100.directColor: true", "-xrm", "XTerm.vt100.cursorBlink: false",
                           "-b", "8", "-e", "bash", "-c", f"printf '\\e[?25l'; cat {path}; sleep 60"])
    time.sleep(1.5)
    full = f"/tmp/{name}-full.png"
    subprocess.run(["import", "-window", "root", full], check=True)
    xt.kill()
    # обрезать по окну xterm: всё, что не цвет фона Xvfb (чёрный), — терминал
    img = Image.open(full).convert("RGB")
    bbox = Image.eval(img, lambda v: 255 if v else 0).getbbox()
    img.crop(bbox).save(os.path.join(OUT, name + ".png"))
    return palette


def page(title, body, theme, font_css):
    bg, fg = theme
    return f"""<!doctype html><html><head><meta charset="utf-8"><title>{title}</title><style>
body{{background:{bg};color:{fg};font:{font_css};margin:24px;-webkit-font-smoothing:antialiased}}
.card{{border:1px solid #8884;border-radius:10px;padding:16px 20px;margin:12px 0;max-width:820px}}
.row{{display:flex;gap:16px;margin:6px 0}} .label{{opacity:1;min-width:190px}}
.grid{{display:grid;grid-template-columns:repeat(3,180px);gap:10px 18px}} .grid div{{border:1px solid #8886;border-radius:6px;padding:6px 10px}}
.msg{{border-radius:14px;padding:8px 14px;margin:8px 0;max-width:520px;border:1px solid #8885}} .me{{margin-left:200px}}
pre{{font:14px/1.5 'DejaVu Sans Mono';margin:0}}
h1{{font-size:22px;margin:0 0 10px}} h2{{font-size:17px;margin:14px 0 6px}} button{{font:inherit;padding:6px 14px}}
</style></head><body>{body}</body></html>"""


def shoot_html(name, title, blocks, theme, font_css, width=1100):
    """blocks: HTML-строки, где секреты — Secret; цвета подставляются при сборке."""
    bg, fg = theme
    rows = [[b] if not isinstance(b, list) else b for b in blocks]
    palette = assign_colors(rows, fg)
    color = {id(s): c for s, c in palette}
    body = ""
    for row in rows:
        for t, sec in parts(row):
            body += f'<span style="color:{color[id(sec)]}">{html.escape(t)}</span>' if sec else t
    path = f"/tmp/{name}.html"
    open(path, "w").write(page(title, body, theme, font_css))
    png = os.path.join(OUT, name + ".png")
    subprocess.run(["chromium", "--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
                    "--disable-font-subpixel-positioning", "--disable-lcd-text", f"--window-size={width},900", f"--screenshot={png}", "file://" + path],
                   check=True, capture_output=True)
    # обрезать пустой низ
    img = Image.open(png).convert("RGB")
    bgc = tuple(int(bg[k:k + 2], 16) for k in (1, 3, 5))
    diff = Image.eval(Image.merge("RGB", [c.point(lambda v, b=b: 255 if v != b else 0) for c, b in zip(img.split(), bgc)]), lambda v: v)
    bb = diff.getbbox()
    if bb:
        img.crop((0, 0, img.width, min(img.height, bb[3] + 24))).save(png)
    return palette


def html_scenes():
    sc = []
    sc.append(("web-account", "Профиль", [
        "<h1>Профиль</h1><div class=card>",
        ["<div class=row><span class=label>Имя</span><span>", S("Name", fio()), "</span></div>"],
        ["<div class=row><span class=label>Почта</span><span>", S("Email", email()), "</span></div>"],
        ["<div class=row><span class=label>Телефон</span><span>", S("Phone", phone()), "</span></div>"],
        ["<div class=row><span class=label>СНИЛС</span><span>", S("Snils", snils()), "</span></div>"],
        ["<div class=row><span class=label>ИНН</span><span>", S("Inn", inn(12)), "</span></div>"],
        "</div><button>Сохранить</button>",
    ]))
    sc.append(("web-checkout", "Оплата", [
        "<h1>Оплата заказа № 48211</h1><div class=card>",
        ["<div class=row><span class=label>Номер карты</span><span>", S("Card", card()), "</span></div>"],
        "<div class=row><span class=label>Срок</span><span>12/28</span></div>",
        ["<div class=row><span class=label>Владелец</span><span>IVAN PETROV</span></div>"],
        "<div class=row><span class=label>Сумма</span><span>4 990,00 ₽</span></div></div>",
    ]))
    sc.append(("web-passport", "Анкета", [
        "<h1>Анкета заявителя</h1><div class=card>",
        ["<div class=row><span class=label>ФИО</span><span>", S("Name", fio()), "</span></div>"],
        ["<div class=row><span class=label>Паспорт</span><span>серия и номер ", S("Passport", f"{R.randrange(10, 99)} {R.randrange(10, 99)} {R.randrange(100000, 999999)}"), "</span></div>"],
        ["<div class=row><span class=label>Счёт для выплат</span><span>", S("BankAccount", "40817810" + rs(12, string.digits)), "</span></div>"],
        ["<div class=row><span class=label>ИНН</span><span>", S("Inn", inn(12)), "</span></div></div>"],
    ]))
    sc.append(("web-github-token", "Personal access tokens", [
        "<h1>Personal access tokens</h1><div class=card>",
        "<p>Make sure to copy your personal access token now. You won’t be able to see it again!</p>",
        ["<pre>", S("Token", "ghp_" + rs(36)), "</pre></div>"],
    ]))
    sc.append(("web-aws-keys", "Access keys", [
        "<h1>Retrieve access keys</h1><div class=card>",
        ["<div class=row><span class=label>Access key</span><span>", S("Token", "AKIA" + rs(16, string.ascii_uppercase + string.digits)), "</span></div>"],
        ["<div class=row><span class=label>Secret access key</span><span>", S("RandomSecret", rs(40, ALNUM + "+/")), "</span></div></div>"],
    ]))
    words = seed(12)
    sd = S("SeedPhrase", "")
    sc.append(("web-seed-grid", "Recovery phrase", [
        "<h1>Your recovery phrase</h1><p>Write these 12 words down in order and keep them somewhere safe.</p><div class=grid>",
        *[["<div>", f"{i + 1}. ", (sd, w), "</div>"] for i, w in enumerate(words)],
        "</div>",
    ]))
    sc.append(("web-wallet", "Кошелёк", [
        "<h1>Кошелёк</h1><div class=card>",
        ["<div class=row><span class=label>Адрес ETH</span><span>", S("Wallet", "0x" + rs(40, "0123456789abcdef")), "</span></div>"],
        ["<div class=row><span class=label>Адрес BTC</span><span>", S("Wallet", btc_legacy()), "</span></div>"],
        ["<div class=row><span class=label>Ключ (WIF)</span><span>", S("CryptoKey", wif()), "</span></div></div>"],
    ]))
    sc.append(("web-chat", "Чат", [
        "<h1>Чат: Семья</h1>",
        "<div class=msg>Мам, пароль от вайфая какой?</div>",
        ["<div class='msg me'>пароль: ", S("Password", rs(10, string.ascii_lowercase + string.digits)), "</div>"],
        ["<div class=msg>Скинь номер карты, переведу</div>"],
        ["<div class='msg me'>", S("Card", card()), "</div>"],
        ["<div class=msg>и позвони потом: ", S("Phone", phone()), "</div>"],
    ]))
    sc.append(("web-chat-en", "Chat", [
        "<h1>#infra</h1>",
        ["<div class=msg>prod db is at ", S("Ip", ipv4()), ", creds in the vault</div>"],
        ["<div class=msg>here, temporary: ", S("UrlCredentials", f"mongodb://admin:{rs(14)}@{host()}:27017"), "</div>"],
        ["<div class=msg>also my key for the staging box: ", S("Token", "sk_live_" + rs(24)), "</div>"],
        "<div class=msg>thanks, will rotate after the release</div>",
    ]))
    sc.append(("web-ide-env", "app/.env", [
        "<h1>app/.env</h1><div class=card><pre>",
        "APP_ENV=production\nAPP_DEBUG=false\n",
        ["SECRET_KEY=", S("Password", rs(32)), "\n"],
        ["DATABASE_URL=", S("UrlCredentials", f"postgres://app:{rs(12)}@db.internal:5432/app"), "\n"],
        ["OPENAI_API_KEY=", S("Token", "sk-proj-" + rs(40, ALNUM + "_-")), "\n"],
        "LOG_LEVEL=info\n</pre></div>",
    ]))
    sc.append(("web-ide-1c", "Подключение 1С", [
        "<h1>Настройки обмена</h1><div class=card><pre>",
        ["Строка подключения: ", S("ConnectionString", f'Srvr="srv-1c";Ref="buh";Usr="{R.choice(LOGINS)}";Pwd="{rs(8)}";'), "\n"],
        "Таймаут: 60\nПовторять: 3\n</pre></div>",
    ]))
    sc.append(("web-ide-code", "client.py", [
        "<h1>client.py</h1><div class=card><pre>",
        "import requests\n\n",
        ["API_KEY = \"", S("Token", "AIza" + rs(35, ALNUM + "_-")), "\"\n"],
        "def fetch(q):\n    return requests.get(URL, params={'q': q, 'key': API_KEY}).json()\n</pre></div>",
    ]))
    sc.append(("web-jwt", "Request", [
        "<h1>Request headers</h1><div class=card><pre>",
        "GET /api/v2/orders HTTP/1.1\nHost: api.example.com\n",
        ["Authorization: Bearer ", S("Token", jwt()), "\n"],
        "Accept: application/json\n</pre></div>",
    ]))
    sc.append(("web-paths", "Отчёт об ошибке", [
        "<h1>Отчёт об ошибке</h1><div class=card><pre>",
        ["FileNotFoundError: ", S("UserPath", f"C:\\Users\\{R.choice(LOGINS)}"), "\\AppData\\Local\\app\\config.json\n"],
        ["  при открытии ", S("UserPath", f"\\\\{R.choice(['fileserver', 'nas01'])}\\share\\buh"), "\\отчёт.xlsx\n"],
        ["Сервер: ", S("Host", host()), "\n</pre></div>"],
    ]))
    # чистые
    sc.append(("web-clean-article", "Статья", [
        "<h1>Как выбрать монитор для работы</h1>",
        "<p>Для работы с текстом удобнее матрица IPS с диагональю 27 дюймов и разрешением 2560×1440. "
        "Частота 60 Гц достаточна, если вы не играете. Обратите внимание на регулировку высоты и наклона.</p>",
        "<p>В 2026 году такие мониторы стоят от 19 990 до 34 990 рублей. Гарантия обычно 3 года.</p>",
    ]))
    sc.append(("web-clean-docs", "Docs", [
        "<h1>Getting started</h1>",
        "<p>Install the package with <code>pip install requests</code>, then import it in your code.</p>",
        "<div class=card><pre>import requests\nr = requests.get('https://example.com/api/v1/items', timeout=10)\nprint(r.status_code, len(r.json()))\n</pre></div>",
        "<p>Version 2.32.3 was released on 2024-05-29. See the changelog for details.</p>",
    ]))
    sc.append(("web-clean-code", "main.rs", [
        "<h1>src/main.rs</h1><div class=card><pre>",
        "use std::collections::HashMap;\n\nfn main() {\n    let mut counts: HashMap&lt;String, usize&gt; = HashMap::new();\n"
        "    for word in std::env::args().skip(1) {\n        *counts.entry(word).or_default() += 1;\n    }\n    println!(\"{counts:?}\");\n}\n</pre></div>",
    ]))
    return sc


def main():
    os.makedirs(OUT, exist_ok=True)
    for f in os.listdir(OUT):
        os.remove(os.path.join(OUT, f))
    xvfb = subprocess.Popen(["Xvfb", ":99", "-screen", "0", "1920x1080x24", "-nolisten", "tcp"])
    os.environ["DISPLAY"] = ":99"
    time.sleep(1)
    n = 0
    meta = {}
    for i, (name, rows) in enumerate(term_scenes()):
        theme, font = THEMES[i % len(THEMES)], FONTS[i % len(FONTS)]
        n += 1
        fname = f"{n:02d}-{name}"
        pal = shoot_term(fname, rows, theme, font)
        meta[fname] = (pal, f"xterm, {font[0]} {font[1]}px, фон {theme[0]}")
    css = ["15px 'DejaVu Sans'", "14px 'Liberation Sans'", "16px 'Noto Sans'", "13px 'DejaVu Sans'"]
    for i, (name, title, blocks) in enumerate(html_scenes()):
        theme = [("#ffffff", "#202124"), ("#1f1f1f", "#e3e3e3"), ("#f6f8fa", "#1f2328")][i % 3]
        n += 1
        fname = f"{n:02d}-{name}"
        pal = shoot_html(fname, title, blocks, theme, css[i % len(css)])
        meta[fname] = (pal, f"Chromium, {css[i % len(css)]}, фон {theme[0]}")
    for fname, (pal, source) in meta.items():
        items = boxes(os.path.join(OUT, fname + ".png"), pal)
        missing = [x["category"] for x in items if not x["rects"]]
        if missing:
            print(f"!! {fname}: не нашёл пикселей секрета {missing}")
        json.dump({"source": source, "items": items}, open(os.path.join(OUT, fname + ".json"), "w"), ensure_ascii=False, indent=1)
        print(f"{fname}: секретов {len(items)}")
    xvfb.kill()


if __name__ == "__main__":
    main()
