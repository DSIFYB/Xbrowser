# Axiom Browser 0.1

Минимальный браузер для Windows 10/11 x64 на Rust и Microsoft Edge WebView2.

## Запуск

Дважды нажмите `launch.cmd`. Готовая программа находится в `dist/AxiomBrowser.exe`. Если EXE отсутствует, запускатель вызывает `cargo run --release`. Для готового EXE нужен только Microsoft Edge WebView2 Runtime, Rust и Visual Studio пользователю не нужны.

Введите адрес и нажмите Enter или Go. Без схемы используется HTTPS; `localhost:порт` открывается через HTTP. Кнопки Back, Forward и Reload управляют текущей страницей. При запуске открывается example.com.

Профиль с cookie хранится в `%LOCALAPPDATA%\AxiomBrowser\Profile`. Обязательных серверов Axiom и телеметрии нет. Страницы напрямую соединяются со своими серверами.

## Сборка

Нужны Rust stable x86_64-pc-windows-msvc, Visual Studio C++ Build Tools, Windows SDK и WebView2 Runtime.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-environment.ps1
cargo test --locked
cargo build --release --locked
New-Item -ItemType Directory -Force dist
Copy-Item target/release/axiom-browser.exe dist/AxiomBrowser.exe
```

## Проверка сайтов

```powershell
cargo run --locked -- --smoke-test
```

Проверка открывает example.com, проверяет заголовок и наличие текста, затем вводит rust-lang.org в адресную строку, проверяет содержимое, назад, вперёд и обновление. Во время проверки меняется размер окна. Код выхода 0 означает пять успешных загрузок с проверкой содержимого, 1 — ошибку или таймаут 60 секунд. Используется отдельный профиль `.smoke-profile`.

## Объём версии

Одно окно и одна страница: адресная строка, HTTPS/HTTP, навигация, изменение размера окна и постоянный профиль. Запрашиваемые разрешения и дополнительные окна запрещены. Интерфейс разрешений, вкладки, история, закладки, экономия памяти, дизайн и оптимизация будут отдельными этапами. Не проверена совместимость каждого сайта, DRM и сложных авторизаций.

Полный технический план находится в `docs/Axiom_Browser_Technical_Plan_v0.1.md`; эта версия реализует начальный прототип согласно сокращённому объёму пользователя.
