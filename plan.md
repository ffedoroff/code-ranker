# План упрощения code-split до модели «только файлы»

> Статус: план (без написания кода). Ветка `explore/files-only`.
> Формат: по каждой области — **Удалить / Изменить / Создать**.

## Контекст

Используются только **файлы + связи + LOC + HK**, а система тащит три уровня
графа, граф вызовов на rust-analyzer и т.д. Цель — упростить во всех смыслах,
**не потеряв связи между файлами** (включая Rust, где зависимости заданы путями
к модулям). Метрики оставляем **полностью**; команды `diff/report/check` — все.

### Зафиксированные решения
- Полный набор метрик → `rust-code-analysis` остаётся (он их и считает).
- Кастомный паблиш `rust-code-analysis-code-split` (tree-sitter 0.26) — при
  полном наборе метрик де-факто необходим; **оставляем**, уход на апстрим — в
  backlog (вариант: разбирать импорты Py/JS парсерами rust-code-analysis и сесть
  на одну версию tree-sitter).
- `diff/report/check` остаются; UI схлопывается к одному уровню.

### Итоговая модель
Один граф файлов. Узлы: `File` (внутренний) + `External` (внешняя либа, глубина
1). Рёбра: `Uses`/`Reexports` между файлами + `Uses {external:true}` к либам.
HK — только по внутренним рёбрам; внешний fan-out хранится отдельно.

---

## 1. code-split-core — модель и сборка снапшота

**Изменить**
- `graph.rs`: `NodeKind` свести к `File` + `External` (убрать
  `Crate/Module/Fn/Method/Impl/Trait`). `EdgeKind` оставить `Uses`/`Reexports`
  (убрать `Calls`; `Contains` убрать или оставить неиспользуемым — см. примечание).
- `graph.rs::Complexity`: добавить поле `fan_out_external: u32` в `Coupling`
  (внешний fan-out, не входит в HK).
- `snapshot.rs::PluginGraphs`: убрать поля `modules` и `functions`, оставить один
  `files: Graph` (минимум правок в JS, читающем `graphs.files`).
- `snapshot.rs`: `relativize_graphs` / `rewrite_ids` — убрать обход
  modules/functions, оставить один граф (строки ~106-108, 156-213).
- `cycles.rs::annotate_graph_cycles` и `hk.rs::annotate_hk`: вызывать на одном
  графе вместо трёх (`hk.rs:8-12`).
- `diff.rs::compare_snapshots`: считать diff по одному уровню (строки ~49-51).

**Удалить**
- `Graph::project(...)` по `node_kinds`/`edge_kinds` — больше не нужно проецировать
  на разные уровни (или оставить для фильтра External/edge-kind — см. примечание).
- Тесты в `graph.rs`, ссылающиеся на `Crate/Module` проекции.

**Примечание (`Contains`):** в files-only между файлами нет контейнмента, поэтому
`Contains`-рёбра по сути исчезают. Решить: совсем убрать `Contains` из `EdgeKind`
или оставить пустым для совместимости. Рекомендация — убрать.

---

## 2. Rust-плагин + удаление sema (главный нетривиальный кусок)

**Удалить**
- Крейт `crates/code-split-sema/` целиком.
- В `Cargo.toml` (workspace) — все зависимости `ra_ap_*` и запись
  `code-split-sema` (минус 33+ крейта из lock).
- `plugin/rust.rs:32-64` — весь блок вызова sema (`want_functions` ветвь).
- В `rust.rs:87-108` — проекции `modules` и `functions`.

**Изменить**
- `plugin/rust.rs`: вместо трёх проекций — построить **file-граф** свёрткой
  Module→File (см. «Создать»). Сигнатуру `run(...)` упростить (убрать
  `want_functions`).
- `code-split-syn` остаётся источником: он уже эмитит Module-узлы с `path`
  (= файл) и `use`-рёбра между модулями. Менять его эмиссию **не обязательно** —
  свёртку делаем постпроходом в core/плагине.

**Создать**
- Функция свёртки `collapse_modules_to_files(graph) -> Graph` (в core или
  rust-плагине):
  1. сгруппировать `Module`-узлы по `path` → один `File`-узел на файл; перенести
     `loc`/`item_count`/`complexity` с файлового модуля (`line=None`); инлайновые
     модули (`line=Some`) схлопнуть в их файл;
  2. построить map `module_id → file_id` (через `node.path`);
  3. каждое `Uses`/`Reexports`-ребро переподвесить на файлы по map; убрать петли
     (внутрифайловые) и дедуплицировать;
  4. внешние крейты (`external:true`, `crate_graph.rs:17`) → один `External`-узел
     на крейт (глубина 1), ребро `Uses {external:true}`; не тащить registry-файлы.

**Критерий приёмки:** число file→file рёбер = числу межфайловых `use`-связей
(минус петли). Это и есть «связи не потеряны».

---

## 3. Python-плагин (`plugin/python.rs`)

**Удалить**
- `add_package_ancestors(...)` (эмиссия `Module`-узлов пакетов, ~строки 207-245).
- Эмиссию `Impl` (классы, ~354-377) и `Fn`/`Method` (~381-430).
- Проекции `modules` и `functions` (~97, 102-110); heuristic-sema блок `Calls`
  (~до строки 92), т.к. функций больше нет.

**Изменить**
- Оставить эмиссию `File`-узлов и file→file `Uses` (~289-343).
- **Переподвесить импорт пакета**: сейчас при импорте `__init__.py` цель =
  `mod:`-узел (`python.rs:329-330`) — направить на `File`-узел самого
  `__init__.py`.
- Проекция → один `File`-граф.

**Создать**
- Эмиссия `External`-узла при нерезолве импорта в проектный файл: один узел на
  top-level пакет (`numpy`, `pkg.sub` → `numpy`/`pkg`), ребро `Uses {external:true}`,
  дедуп по имени.

---

## 4. JS/TS-плагин (`plugin/javascript.rs`)

Симметрично Python.

**Удалить**
- `add_package_ancestors(...)` (`Module` для директорий, ~240-295).
- Эмиссию `Impl`/`Fn`/`Method` (~404-490); heuristic-sema `Calls`.
- Проекции `modules`/`functions` (~94, 99-107).

**Изменить**
- Оставить `File` + file→file `Uses` (~347-395). Импорт директории/`index.*`,
  целящийся в `mod:`-узел, переподвесить на `File`-узел `index.*`.
- Проекция → один `File`-граф.

**Создать**
- `External`-узел на 3rd-party импорт: один узел на пакет (учесть scoped
  `@scope/pkg` и bare specifiers vs относительные пути), ребро `Uses {external:true}`.

---

## 5. code-split-complexity

**Изменить**
- Цель аннотации — только `File`-узлы (Rust: файловый узел после свёртки; Py/JS:
  `File`). Убрать матчинг `Fn`/`Method`/`Impl` (таблицы 2 и 3 в `analyze_extensions`).
- Для Rust: аннотировать ДО или ПОСЛЕ свёртки модулей — согласовать порядок
  (проще: аннотировать файловые модули, затем свёртка переносит `complexity`).

**Удалить**
- Не требуется удалять сам крейт/движок (метрики оставляем). Только мёртвый код
  матчинга функций.

---

## 6. CLI — `config.rs` и `main.rs`

**Изменить (`config.rs`)**
- `ThresholdRules` (~118-129): убрать поля `module`, `function`; оставить `file`.
- Парсинг scope (~407-410): убрать кейсы `module`/`function`; на их указание —
  понятная ошибка.
- `apply_ignore` (~464-479), `apply_cycle_rules_graph` (~630-632),
  `check_graph_violations` (~832-880): работать по одному графу; убрать кейсы
  `"modules"`/`"functions"` из маппинга scope→bucket.
- `RULES`-каталог: менять не нужно (правила метрик scope-агностичны).

**Изменить (`main.rs`)**
- Убрать логику `want_modules`/`want_functions` и зануление графов (~320-326);
  оставить только файловый граф.
- `annotate_stats` (~342-344) — на одном графе.
- Печать scope-значений (`--suggest-config`, ~553-608) — только scope `file`.

**Удалить**
- Флаги/опции, относящиеся к выбору уровня modules/functions, если такие есть в
  `clap`-структуре (`--graph modules|files|functions` → оставить только files
  или убрать флаг).

---

## 7. HTML/UI ассеты (`crates/code-split-cli/src/assets/`)

> Пользователь переработает отчёты сильно сам — здесь минимум для консистентности
> с одноуровневой моделью + поддержка external.

**Изменить**
- `index.html` (~37-100): убрать переключатель уровней и секции modules/functions;
  оставить одну секцию (Files) либо убрать переключатель совсем.
- `state.js` (~10): дефолт `graph: 'files'`.
- `summary.js` (~66): `levels = ['files']`.
- `diff.js` (~33, 108): циклы по уровням → только `files`.
- `app.js` (~59-70): убрать логику скрытия Files-таба (он всегда есть).
- `node-table.js` (~47-51): колонки только для файлов (включая HK, fan-in/out,
  external fan-out).
- `layout.js` (~41-47): **добавить раскраску** `n.external` отдельным цветом
  (сейчас все узлы одного цвета `N_FILL`/`N_COLOR`).
- **Перестать прятать external** в основном виде: `app.js:14`,
  `node-table.js:205`, `summary.js:2`, `diff.js:37-38` — сейчас всюду
  `.filter(n => !n.external)`; для основного графа показывать external (но,
  возможно, исключать из таблицы/средних — решить отдельно).

**Удалить**
- Логику, специфичную для modules/functions DOT-стилей в `layout.js`.
- Промпты, завязанные на modules/functions, в `export-popup.js` (генератор
  промптов оставляем как фичу; чистим только уровни).

---

## 8. External-зависимости глубины 1 (сквозная фича)

**Создать/Изменить (суммарно из п.2-4,7)**
- `NodeKind::External` (или продолжить использовать `external:true` на узле —
  выбрать одно; рекомендация: явный kind для files-only ясности).
- Эмиссия одного узла на внешнюю либу + ребро `external:true` во всех трёх
  плагинах (Rust почти готов; Py/JS — новый код).
- Раскраска в SVG (`layout.js`).
- HK: см. п.9.

---

## 9. HK / stats

**Изменить**
- `hk.rs::annotate_graph_hk` (~23-35): при наборе fan-in/out **пропускать рёбра с
  `external` / во внешний узел**; считать их отдельно в `fan_out_external`.
- `stats.rs::annotate_stats`: при усреднении решить, включать ли External-узлы
  (рекомендация: исключать — это либы, не файлы проекта).

---

## 10. Документация

**Изменить**
- `docs/PRD.md`, `docs/DESIGN.md`, `docs/NODE_SCHEMA.md`, `docs/CLI.md`,
  `docs/ERRORS.md`, `docs/config.md`, `README.md`, `docs/COMPARISON.md`,
  `code-split.toml`: убрать упоминания трёх уровней, графа функций, sema/ra_ap_*,
  scope `module`/`function`; описать одноуровневую модель, External глубины 1,
  раскраску, правило HK (внутренние рёбра). Обновить пример снапшота (`graphs`
  с одним ключом).

---

## 11. Тесты

**Удалить/Изменить**
- Проверки на `graphs.modules`/`graphs.functions` и kind `Module/Fn/Method`:
  `snapshot.rs` (~378-379, 516-561), `cycles.rs` (~407-411),
  `config.rs` (~1275-1438), `python.rs` (~992-995), `javascript.rs` (~1142-1145).
- `hk.rs`-тесты переписать на `NodeKind::File`.

**Создать**
- Тест свёртки Rust Module→File: число file→file рёбер = межфайловые `use` минус
  петли (главный критерий).
- Тест эмиссии External-узлов в Python/JS (один узел на либу, дедуп, `external:true`).
- Тест, что HK игнорирует external-рёбра, а `fan_out_external` их считает.

---

## 12. Cargo / workspace

**Удалить**
- `crates/code-split-sema` из `members` и workspace-deps; все `ra_ap_*` деп-записи.

**Изменить**
- (Опционально, упрощение структуры) рассмотреть свёртку `code-split-complexity`
  в core или cli — но это не обязательно; оставить на усмотрение.

---

## Порядок работ (предлагаемый)
1. core: модель (`NodeKind`/`EdgeKind`/`PluginGraphs`/Complexity), свёртка-утилита.
2. Rust-плагин + удаление sema/ra_ap_* → прогнать на реальном Rust-проекте,
   проверить критерий «связи целы».
3. Python, затем JS/TS (включая External-узлы).
4. complexity (только File), hk/stats (external-развязка).
5. config + main (scope `file`).
6. UI-ассеты (один уровень + раскраска external).
7. тесты, доки.

## Верификация
- `cargo test --workspace`, `cargo clippy`.
- `code-split report <rust-проект>` и `<py/js-проект>`: снапшот с одним
  `graphs.files`; на каждом `File`-узле есть loc.*, halstead.*, MI/SEI,
  fan_in/out, hk; присутствуют `External`-узлы с `external:true`; HK у файлов с
  внешними импортами не раздут.
- `code-split diff` / `check` работают на одноуровневой модели.
- Открыть HTML: external-узлы видны и окрашены отдельно.

## Backlog (вне основного объёма)
- Уход с кастомного `rust-code-analysis-code-split` на апстрим (консолидация
  tree-sitter).
- Возможная свёртка крейтов 5→3/2.
