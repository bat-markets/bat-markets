# bat-markets — `blueprint.md`

**Status:** Draft  
**Audience:** maintainers, contributors, early adopters  
**Scope:** open-source Rust exchange engine  
**Primary venues for initial support:** Binance, Bybit  
**Primary product focus for initial support:** linear futures / perpetuals  
**Primary objective:** a unified, low-latency, futures-first Rust core that stays small, predictable, extensible, and honest about exchange differences.

---

## 1. Что такое bat-markets

`bat-markets` — это **headless Rust exchange engine**.

Это не UI, не брокерская платформа, не KYC/AML-система, не готовый custodial backend и не монолит “на все случаи жизни”.  
Это ядро, на котором можно строить:

- market data consumers,
- screeners,
- trading apps,
- execution services,
- monitoring tools,
- later: broker-like user flows поверх отдельного backend.

Главная идея проекта:

> **Сделать Rust-first альтернативу подходу unified exchange API, но без ложной унификации, без тяжёлой архитектуры, без “stringly typed” моделей и без скрытых костылей.**

---

## 2. Цели проекта

### 2.1 Основные цели

1. **Максимально предсказуемое поведение**
   - единый публичный API там, где это реально полезно;
   - exchange-specific поведение не скрывается магией, а выносится в явный native-слой.

2. **Низкая задержка и высокая пропускная способность**
   - futures-first;
   - минимальное количество hops в hot path;
   - отдельный быстрый контур для market data;
   - lossless private state path;
   - минимум лишних аллокаций и преобразований.

3. **Простая расширяемость**
   - сначала Binance + Bybit;
   - затем новые биржи без переделки ядра;
   - модульная структура workspace;
   - capability-driven design.

4. **Чистый open-source продукт**
   - docs-first;
   - понятная модель версионирования;
   - минимально необходимая публичная поверхность;
   - внутренние детали не замораживаются раньше времени.

5. **Фундамент под более широкий функционал в будущем**
   - spot,
   - asset flows,
   - transfers,
   - convert,
   - deposits / withdrawals,
   - broker / partner scenarios,
   - но без того, чтобы первая версия тонула в лишней сложности.

---

## 3. Не-цели

На старте `bat-markets` **не** должен:

- быть “полной заменой всего CCXT”;
- покрывать 20+ бирж;
- закрывать весь private wealth / custody / compliance stack;
- включать базу данных внутрь ядра;
- включать HTTP API сервер внутрь ядра;
- тащить GUI concerns в библиотеку;
- притворяться, что все биржи одинаковые;
- строиться поверх giant `HashMap<String, Value>` моделей;
- генерироваться целиком из OpenAPI и жить как codegen jungle.

---

## 4. Проектные принципы

### 4.1 Unified where stable, native where valuable

Нужно унифицировать только то, что:
- действительно совпадает по смыслу,
- полезно для 80% прикладного кода,
- не требует лжи пользователю.

Всё остальное уходит в `native()`.

### 4.2 Futures-first

Первая боевая версия проекта оптимизируется под:
- Binance linear futures,
- Bybit linear futures / perpetuals.

Именно это направление должно определять первую форму публичного API, testing strategy и performance priorities.

### 4.3 Fast path отдельно от ergonomic path

Одна из главных архитектурных ошибок универсальных библиотек — одна доменная модель на всё:
- для стримов,
- для REST,
- для внутреннего состояния,
- для прикладного API,
- для native extensions.

В `bat-markets` так делать нельзя.

Должны существовать **три уровня представления данных**:

1. **Native / Raw**
2. **Fast normalized**
3. **Unified ergonomic**

Это не усложнение. Это и есть способ убрать костыли.

### 4.4 Минимум публичной поверхности

В `0.x` стабильными должны быть:
- high-level facade,
- основные типы,
- error taxonomy,
- capability model.

Внутренние adapter contracts, transport details и state internals не должны замораживаться слишком рано.

### 4.5 Ручные адаптеры, а не “автогенерированный монолит”

Core adapters должны быть handwritten и audit-friendly.

Разрешается:
- локальный codegen для тестов,
- генерация capability docs,
- генерация examples/snippets,
- генерация payload fixtures index.

Не разрешается:
- строить весь runtime contract на автогенерированном клиенте.

### 4.6 No hidden global state

Никаких:
- скрытых синглтонов,
- глобальных rate limiters,
- глобального runtime,
- глобальных mutable registries.

Состояние клиента должно быть локально и явно управляемо.

### 4.7 Lossless private state, pragmatic public data

- Public market data может поддерживать controlled coalescing.
- Private streams и account/order state должны быть lossless с точки зрения логики состояния.

---

## 5. Архитектурная формула проекта

`bat-markets` строится как:

> **Futures-first, headless, capability-driven Rust exchange kernel**  
> с тремя API-слоями, тремя execution lanes и двумя exchange adapters.

### 5.1 Три API-слоя

#### A. Native / Raw API

Используется там, где важны:
- биржевые особенности,
- специальные order semantics,
- уникальные asset/account операции,
- low-level stream access,
- всё то, что нельзя честно вписать в unified contract.

#### B. Fast normalized API

Используется там, где нужна скорость:
- tickers,
- trades,
- order book updates,
- executions,
- health,
- lightweight analytics feeds.

Здесь модель должна быть:
- компактной,
- без лишних аллокаций,
- без “красивой, но тяжёлой” абстракции.

#### C. Unified ergonomic API

Используется для:
- приложений,
- higher-level сервисов,
- общего интеграционного кода,
- стандартных торговых и account операций.

---

## 6. Репозиторий и workspace

Репозиторий называется **`bat-markets`**.

### 6.1 Структура workspace

```text
bat-markets/
  Cargo.toml
  README.md
  LICENSE-APACHE
  LICENSE-MIT
  CHANGELOG.md
  CONTRIBUTING.md
  SECURITY.md
  rust-toolchain.toml

  crates/
    bat-markets/             # public facade crate
    bat-markets-core/        # internal core
    bat-markets-binance/     # Binance adapter
    bat-markets-bybit/       # Bybit adapter
    bat-markets-testing/     # fixtures, conformance, soak tests

  docs/
    blueprint.md
    architecture/
      layers.md
      execution-lanes.md
      state-model.md
      symbols-and-instruments.md
    adr/
      0001-project-shape.md
      0002-three-api-levels.md
      0003-futures-first-v0.md
      0004-capabilities.md
      0005-no-db-in-core.md
    capability-matrix.md
    error-model.md
    versioning-policy.md

  examples/
    market_ticker.rs
    market_stream.rs
    private_orders.rs
    private_positions.rs
    native_binance.rs
    native_bybit.rs

  benches/
    stream_decode.rs
    fast_normalize.rs
    state_apply.rs

  scripts/
    check.sh
    release.sh
```

### 6.2 Публикуемые crates

На crates.io публикуются:

- `bat-markets`
- `bat-markets-binance`
- `bat-markets-bybit`

На старте **не публикуются** или публикуются позже:
- `bat-markets-core`
- `bat-markets-testing`

Причина: не цементировать внутреннюю архитектуру раньше времени.

---

## 7. Публичный crate `bat-markets`

`bat-markets` — это единственная точка входа, которая должна ощущаться как “продукт”.

Он:
- даёт builder/facade,
- реэкспортит ключевые типы,
- предоставляет единый public API,
- включает feature flags для конкретных venues,
- содержит docs и examples.

### 7.1 Публичные модули

```text
bat_markets::
  client
  market
  stream
  trade
  position
  account
  asset
  health
  native
  types
  errors
  capabilities
  config
```

### 7.2 Базовый ergonomic API

```rust
use bat_markets::{BatMarkets, Venue, Product};

let client = BatMarkets::builder()
    .venue(Venue::Binance)
    .product(Product::LinearUsdt)
    .build()
    .await?;

let caps = client.capabilities();

let ticker = client.market().ticker("BTC/USDT:USDT").await?;
let positions = client.position().list().await?;
```

### 7.3 Native API

```rust
let native = client.native();

match client.venue() {
    Venue::Binance => {
        let binance = native.binance()?;
        // binance-specific operation
    }
    Venue::Bybit => {
        let bybit = native.bybit()?;
        // bybit-specific operation
    }
}
```

### 7.4 Правило публичного API

Публичный unified API должен быть:
- достаточно узким,
- достаточно предсказуемым,
- достаточно типизированным,
- и при этом не пытаться поглотить все exchange-specific нюансы.

---

## 8. Внутренний crate `bat-markets-core`

`bat-markets-core` — это закрытая архитектурная опора проекта.

Он не должен восприниматься как отдельный продукт.  
Его задача — дать устойчивый внутренний фундамент.

### 8.1 Что в нём живёт

- domain types;
- transport abstraction;
- ws runtime;
- signer abstraction;
- rate limiting;
- retry policy;
- time sync / clock skew;
- health model;
- state engine;
- reconciliation engine;
- errors;
- config;
- capability model;
- symbol/instrument canonicalization.

### 8.2 Что в нём не живёт

- exchange-specific endpoint knowledge;
- generated REST clients как public contract;
- UI logic;
- DB logic;
- broker / account management;
- business workflows outside exchange engine.

### 8.3 Внутренние модули

```text
bat-markets-core/
  src/
    auth/
    capability/
    clock/
    config/
    error/
    health/
    instrument/
    model/
    normalize/
    rate_limit/
    retry/
    signer/
    state/
    stream/
    transport/
    util/
```

---

## 9. Адаптеры бирж

### 9.1 Binance adapter

`bat-markets-binance` отвечает только за Binance:

- endpoint routing;
- auth/signing details;
- REST request/response models;
- WS public/private stream models;
- mapping в fast/unified models;
- exchange-specific quirks;
- native API surface;
- capability declaration;
- recovery/reconcile specifics.

### 9.2 Bybit adapter

`bat-markets-bybit` отвечает только за Bybit:

- account/product routing;
- `category` semantics;
- REST and WS models;
- mapping;
- native API surface;
- capability declaration;
- reconcile specifics;
- exchange-specific quirks.

### 9.3 Важное правило

**Адаптер не должен знать про приложение.**  
Он знает только:
- свою биржу,
- внутренний core contract,
- свои native особенности,
- mapping в public types.

---

## 10. Adapter contract

Низкоуровневый adapter trait в `0.x` должен быть **sealed/internal**.

Это критично.

Если стабилизировать его слишком рано, проект заморозит случайную форму внутренней архитектуры и будет вынужден тащить её долгие годы.

### 10.1 Что стабилизируется публично

- public facade,
- unified types,
- capability model,
- error taxonomy,
- config primitives.

### 10.2 Что не стабилизируется рано

- internal adapter trait,
- internal transport abstractions,
- internal state trait structure,
- internal parser pipeline.

---

## 11. Доменные модели

### 11.1 Базовые сущности

Минимальный общий набор типов:

- `Venue`
- `Product`
- `MarketType`
- `InstrumentId`
- `AssetCode`
- `OrderId`
- `ClientOrderId`
- `PositionId`
- `RequestId`
- `TradeId`
- `OrderType`
- `Side`
- `TimeInForce`
- `MarginMode`
- `PositionMode`
- `OrderStatus`
- `TriggerType`
- `Balance`
- `Position`
- `Order`
- `Execution`
- `FundingRate`
- `OpenInterest`
- `Ticker`
- `TradeTick`
- `BookTop`
- `BookDelta`
- `Kline`

### 11.2 Typed identifiers

Нельзя строить API вокруг “просто строк”.

Все ключевые идентификаторы должны быть типизированы.

Например:
- `AssetCode`
- `InstrumentId`
- `OrderId`
- `ClientOrderId`

### 11.3 Символы и инструменты

Важное разделение:

- **display symbol** — для пользователя;
- **canonical symbol** — для unified layer;
- **native symbol** — для конкретной биржи;
- **instrument key** — внутренняя typed identity.

`InstrumentId` — это источник истины.  
Строковый символ — лишь удобная форма ввода/вывода.

### 11.4 Числовые типы

Запрещено строить public/state API на `f64`.

Правило:

- в hot path можно использовать **quantized integer representation**;
- в public ergonomic API — typed numeric wrappers;
- округление и квантование всегда должны быть явными и опираться на `InstrumentSpec`.

### 11.5 InstrumentSpec

Для каждого инструмента нужен `InstrumentSpec`, включающий:

- venue,
- product,
- canonical symbol,
- native symbol,
- base,
- quote,
- settle,
- contract size,
- tick size,
- step size,
- min qty,
- min notional,
- price precision,
- qty precision,
- leverage limits,
- support flags,
- status.

---

## 12. Capability model

### 12.1 Зачем нужен CapabilitySet

Capability model нужен, чтобы:
- не лгать пользователю насчёт одинаковости бирж;
- не плодить `Option<T>` и не прятать ограничения глубоко в runtime;
- позволить приложению заранее понять, что поддерживается.

### 12.2 CapabilitySet

Примерная структура:

```text
CapabilitySet
  market_public
  market_stream_public
  private_streams
  trade_create
  trade_cancel
  trade_get
  trade_list_open
  trade_history
  position_read
  account_read
  leverage_set
  margin_mode_set
  hedge_mode
  asset_read
  asset_transfer
  asset_withdraw
  convert
  native_fast_stream
  native_special_orders
```

### 12.3 Правила

- capability проверяется быстро и без сетевого вызова;
- capability отражает реальное покрытие адаптера;
- отсутствие capability — это норма, а не ошибка дизайна.

---

## 13. Три execution lanes

Это один из ключевых элементов архитектуры.

### 13.1 Public Market Data Lane

Для:
- tickers,
- trades,
- order books,
- klines,
- funding,
- OI,
- public health signals.

Особенности:
- максимально короткий путь от сокета до подписчика;
- controlled coalescing допустим;
- heavy domain normalization запрещён в hot path.

### 13.2 Private State Lane

Для:
- orders,
- executions,
- balances,
- positions,
- private account events.

Особенности:
- логически lossless;
- sequence-aware;
- включён reconcile pipeline;
- никакого uncontrolled dropping.

### 13.3 Command Lane

Для:
- create order,
- cancel order,
- get order,
- set leverage,
- set margin mode,
- позже: transfer / convert / withdraw.

Особенности:
- свой rate limiter;
- свой retry policy;
- idempotency;
- timeout classification;
- clear mapping на `UnknownExecution` vs definite failure.

---

## 14. Потоки данных

### 14.1 Public path

```text
socket
  -> decode
  -> fast normalize
  -> optional state hook
  -> fan-out to subscribers
```

### 14.2 Private path

```text
socket
  -> decode
  -> sequence/order checks
  -> normalize
  -> apply to state
  -> emit state/event updates
  -> periodic reconcile
```

### 14.3 Command path

```text
request
  -> validate against InstrumentSpec
  -> sign
  -> rate limit
  -> send
  -> decode
  -> classify
  -> update state hint
  -> wait for stream confirmation / reconcile if needed
```

---

## 15. Transport layer

### 15.1 Общие требования

Transport layer должен быть:
- async-only;
- runtime-safe;
- тестируемым;
- заменяемым без public API breakage;
- максимально тонким.

### 15.2 Что transport обязан уметь

- HTTP request/response;
- WebSocket lifecycle;
- ping/pong / heartbeat;
- reconnect policies;
- TLS configuration;
- timeout handling;
- DNS/connect/read/write classification;
- backpressure signals;
- header/auth injection;
- request tracing hooks.

### 15.3 Что transport не должен делать

- знать о торговых моделях;
- знать о позиции/балансе;
- скрывать exchange-specific errors;
- сам решать бизнес-политику retry.

### 15.4 HTTP/WS abstraction policy

Внешний public API не должен зависеть от конкретной реализации HTTP/WS клиента.

Внутри `0.x` можно менять реализацию transport stack без semver promises, если:
- не ломается public facade;
- не меняются public domain types;
- не ломается capability contract.

---

## 16. Signer model

Секреты не должны растекаться по коду как `String`.

### 16.1 Абстракция

Нужен `Signer` trait или эквивалентная sealed interface.

Цель:
- ядро знает, как запросить подпись;
- ядро не обязано знать, где хранится секрет.

### 16.2 Базовые реализации

- `MemorySigner`
- `EnvSigner`
- позже: `KeychainSigner`
- позже: `RemoteSigner`

### 16.3 Правила

- секреты не логируются;
- signature inputs могут трассироваться только в safe/redacted форме;
- повторно используемые signed requests не кэшируются небезопасно.

---

## 17. State engine

### 17.1 Почему state engine нужен с первого дня

Даже если библиотека кажется “просто transport wrapper”, без state engine она быстро превращается в хаос:

- order status расходится;
- position snapshots устаревают;
- private stream после reconnect нужно склеивать;
- временная потеря ack/response делает торговый path нечётким.

### 17.2 Что хранит state engine

Минимально:

- instruments registry;
- order state;
- execution ledger fragment;
- positions;
- balances;
- stream watermarks / sequence markers;
- health indicators;
- snapshot age.

### 17.3 Правило про storage

В ядре — **только in-memory state**.

Никакой встроенной БД.

Если внешнему приложению нужно persistence:
- оно экспортирует snapshots/events наружу;
- оно само выбирает storage.

### 17.4 Snapshot + stream + reconcile

Правильная модель:

1. bootstrap snapshot,
2. stream updates,
3. periodic reconcile,
4. divergence detection,
5. repair.

---

## 18. Reconciliation engine

### 18.1 Назначение

Reconcile engine нужен для:
- восстановления после reconnect;
- проверки uncertain command outcomes;
- сверки состояния после burst volatility;
- устранения drift между REST и stream truth.

### 18.2 Когда он срабатывает

- после reconnect;
- после detected sequence gap;
- после `UnknownExecution`;
- по таймеру;
- по явному запросу.

### 18.3 Выходы reconcile engine

- state repaired;
- state still uncertain;
- divergence event emitted;
- health degraded.

---

## 19. Health model

### 19.1 Почему health — часть ядра, а не внешняя надстройка

Если библиотека будет использоваться для:
- screeners,
- trading apps,
- execution services,

то информация о здоровье соединения и качества состояния должна быть доступна из ядра.

### 19.2 HealthReport

Минимальный набор:

- `rest_ok`
- `ws_public_ok`
- `ws_private_ok`
- `clock_skew_ms`
- `last_public_msg_at`
- `last_private_msg_at`
- `reconnect_count`
- `snapshot_age_ms`
- `state_divergence`
- `degraded_reason`

### 19.3 Гарантия

Health должен быть:
- cheap to query;
- snapshot-based;
- пригоден и для UI, и для автологики.

---

## 20. Error taxonomy

Нужна ясная и строгая модель ошибок.

### 20.1 Категории

- `ConfigError`
- `TransportError`
- `Timeout`
- `RateLimited`
- `AuthError`
- `PermissionDenied`
- `Unsupported`
- `DecodeError`
- `ExchangeReject`
- `UnknownExecution`
- `StateDivergence`
- `ComplianceRequired`
- `TemporaryUnavailable`

### 20.2 Принципы

- ошибки должны быть пригодны для machine handling;
- retriable vs terminal должно быть различимо;
- exchange-specific code/message сохраняются как данные, но не заменяют taxonomy;
- `UnknownExecution` — отдельный класс, а не “какой-то timeout”.

### 20.3 Error context

Каждая ошибка, где это безопасно, должна уметь содержать:
- venue,
- product,
- endpoint/operation,
- request id,
- native code,
- classification,
- retriable flag.

---

## 21. Native layer

### 21.1 Зачем нужен native layer

Чтобы unified layer не превращался в ложь.

### 21.2 Что там живёт

- Binance-specific endpoints
- Bybit-specific endpoints
- special order flags
- special account/asset flows
- low-level stream handles
- venue-specific metadata

### 21.3 Правило

Native layer:
- не должен ломать ergonomics unified layer;
- не должен засорять базовые типы;
- должен быть максимально прямолинейным.

---

## 22. Fast normalized layer

### 22.1 Назначение

Это слой для:
- screeners,
- low-latency analytics,
- internal data fan-out,
- дешёвых subscriber pipelines.

### 22.2 Типы

Минимальный набор:

- `FastTicker`
- `FastTrade`
- `FastBookTop`
- `FastBookDelta`
- `FastKline`
- `FastExecution`
- `FastHealth`

### 22.3 Ограничения

- никаких тяжёлых nested моделей;
- никаких string copies без необходимости;
- никаких “универсальных extensible JSON fields” в hot path.

---

## 23. Unified ergonomic layer

### 23.1 Назначение

Для:
- большинства приложений,
- CLI/tools,
- интеграционного кода,
- higher-level сервисов.

### 23.2 Модули

- `market()`
- `stream()`
- `trade()`
- `position()`
- `account()`
- `asset()`
- `health()`
- `native()`

### 23.3 Принцип API

API должен быть:
- boring,
- explicit,
- discoverable,
- documented by examples.

Нельзя строить его на “магии” и implicit guessing.

---

## 24. Конфигурация

### 24.1 BatMarketsConfig

Конфигурация должна быть структурированной, а не в виде набора строковых параметров.

Примерно:

- venue
- product
- auth config
- network endpoints
- testnet/demo mode
- timeouts
- rate limit policy
- reconnect policy
- state policy
- health thresholds
- tracing hooks

### 24.2 Конфигурационный принцип

- sensible defaults;
- no hidden behavior;
- everything performance-critical is configurable;
- but not every internal knob is public on day one.

---

## 25. Performance rules

Это обязательные правила проекта.

### 25.1 Общие правила

1. Не использовать `f64` в state/public domain.
2. Не делать лишних string copies.
3. Не нормализовать больше, чем нужно.
4. Не тащить dynamic dispatch в hot path без причины.
5. Не использовать глобальные mutex-узкие места.
6. Не делать broadcast giant objects.
7. Не прятать expensive parsing за удобным API.
8. Не оптимизировать вслепую — только после benchmarks.

### 25.2 Hot path budget thinking

Для public data path:
- каждое лишнее преобразование должно быть оправдано;
- каждый hop должен быть виден архитектурно;
- при сомнении выбирается меньший путь.

### 25.3 Memory policy

- prefer borrowing or compact owned forms;
- small typed ids вместо длинных строк в state;
- pooling только после измерений;
- no speculative object pools “на всякий случай”.

---

## 26. Надёжность и отказоустойчивость

### 26.1 Что значит отказоустойчивость в контексте `bat-markets`

Не “никогда не падает”, а:

- чётко классифицирует деградацию;
- восстанавливается после типовых сетевых сбоев;
- умеет переходить в degraded mode;
- не теряет логическую целостность состояния;
- сообщает приложению, когда truth uncertain.

### 26.2 Degraded modes

Минимально должны существовать состояния:

- `Healthy`
- `DegradedPublicStream`
- `DegradedPrivateStream`
- `ReconcileRequired`
- `CommandUncertain`
- `ReadOnlySafe`
- `Disconnected`

### 26.3 Правила

- order path не маскирует uncertain outcome как success;
- private state drift не скрывается;
- public stream backlog может coalesce, private stream backlog — нет.

---

## 27. Security model

### 27.1 Базовые принципы

- секреты не логируются;
- secret material изолирован в signer layer;
- dangerous operations feature-gated;
- redaction по умолчанию;
- auditability важнее “магического удобства”.

### 27.2 Feature flags безопасности

Позже должны существовать feature flags вида:

- `private-trading`
- `asset-read`
- `asset-write`
- `dangerous-withdrawals`

### 27.3 Политика логирования

Логируются:
- operation names,
- venue,
- timing,
- classification,
- health changes.

Не логируются:
- secret values,
- raw signed payloads без redaction,
- sensitive headers,
- full private account dumps по умолчанию.

---

## 28. Версионирование и стабилизация

### 28.1 Фаза `0.x`

В `0.x` проект имеет право менять:
- внутренние contracts,
- transport choices,
- state internals,
- non-essential config details.

Но должен стараться удерживать форму:
- facade,
- key types,
- modules,
- error categories,
- capabilities.

### 28.2 Фаза `1.0`

`1.0` возможна только когда стабилизированы:

- unified market/trade/account/position API,
- symbol/instrument model,
- error taxonomy,
- capability contract,
- futures-first adapter coverage,
- reconnection and reconcile semantics.

---

## 29. Документация

### 29.1 Обязательные документы

- `README.md`
- `docs/blueprint.md`
- `docs/capability-matrix.md`
- `docs/error-model.md`
- `docs/versioning-policy.md`
- `docs/adr/*`

### 29.2 README

README отвечает на вопросы:
- что это за проект;
- для кого он;
- что поддерживается сейчас;
- чего он не пытается делать;
- как начать за 5 минут;
- как выглядит unified vs native API.

### 29.3 ADR discipline

Любое важное архитектурное решение фиксируется в ADR.

Минимальный список:
- project shape,
- three API levels,
- futures-first,
- capability model,
- no DB in core.

---

## 30. Testing strategy

`bat-markets-testing` — обязательная часть проекта.

### 30.1 Уровни тестов

1. **Unit tests**
   - small pure logic,
   - quantization,
   - symbol parsing,
   - error classification.

2. **Fixture decode tests**
   - raw payload -> parsed model -> normalized output.

3. **Conformance tests**
   - один и тот же unified contract для Binance и Bybit.

4. **State apply tests**
   - stream events -> state transitions.

5. **Reconnect tests**
   - disconnect/reconnect/gap/reconcile scenarios.

6. **Command uncertainty tests**
   - timeout / unknown ack / reconcile.

7. **Soak tests**
   - long-running stream stability.

8. **Benchmarks**
   - decode,
   - normalize,
   - state apply,
   - fan-out.

### 30.2 Golden fixtures

Должны храниться replayable fixtures:
- public WS payloads,
- private WS payloads,
- REST snapshots,
- error payloads,
- uncertain outcome scenarios.

### 30.3 Live integration tests

Live tests не должны быть обязательными для каждого `cargo test`.

Они:
- feature-gated,
- secret-gated,
- CI-optional,
- non-blocking для обычных contributors.

---

## 31. CI / quality gates

### 31.1 Базовые quality gates

- `cargo fmt --check`
- `cargo clippy` без игнорирования серьёзных предупреждений
- `cargo test`
- doc tests
- benchmark smoke checks
- public API docs coverage review
- semver checks for public crates

### 31.2 Release gates

Релиз невозможен без:
- updated CHANGELOG,
- capability matrix update,
- examples pass,
- docs build,
- compatibility notes.

---

## 32. Лицензия и governance

### 32.1 Лицензирование

Рекомендуемая модель:
- `MIT OR Apache-2.0`

### 32.2 Governance model

На старте достаточно:
- core maintainers,
- required review on public API changes,
- ADR for structural changes,
- issue labels by area:
  - market
  - stream
  - trade
  - state
  - binance
  - bybit
  - docs
  - perf
  - reliability

---

## 33. Feature flags

Feature flags должны быть coarse-grained и понятными.

### 33.1 Рекомендуемые feature flags на старте

- `binance`
- `bybit`
- `private-trading`
- `metrics`
- `serde`

### 33.2 Чего не должно быть

Не нужно делать десятки микрофлагов ради каждого endpoint.

---

## 34. Что должно войти в первую боевую версию

Первая версия должна быть **маленькой, но правильной**.

### 34.1 Рекомендуемая первая боевая версия: `0.1.0`

#### Поддерживаемые venues/products

- Binance linear futures
- Bybit linear futures / perpetuals

#### Поддерживаемые модули

**Market**
- load instruments
- ticker
- recent trades
- book top / book stream
- klines
- funding rate
- open interest

**Stream**
- public streams
- private streams

**Trade**
- create order
- cancel order
- get order
- list open orders
- list executions / fills

**Position**
- list positions
- set leverage
- set margin mode

**Account**
- read balances
- read account summary

**Health**
- current health snapshot
- subscribe to health changes

**Native**
- native low-level access for each venue

### 34.2 Что сознательно не входит в `0.1.0`

- spot trading
- deposits
- withdrawals
- internal transfers
- convert
- options
- earn / lending
- portfolio margin abstractions
- broker / partner flows
- account center features
- DB/persistence inside core

### 34.3 Почему именно так

Это:
- соответствует futures-first стратегии,
- резко снижает сложность,
- даёт шанс сделать действительно качественное ядро,
- и не закрывает дорогу к будущему расширению.

---

## 35. Roadmap

### 35.1 `0.1.x` — Futures-first stable core

Цель:
- довести Binance/Bybit linear futures до качества production-grade engine core.

Фокус:
- correctness,
- reconnect,
- reconcile,
- private streams,
- performance benches,
- docs.

### 35.2 `0.2.x` — Spot-ready architecture extension

Добавить:
- market spot metadata,
- spot public market data,
- optional spot trading foundation.

Фокус:
- не ломать futures API;
- расширять capability matrix, а не расползаться хаотично.

### 35.3 `0.3.x` — Asset read foundation

Добавить:
- asset read primitives,
- deposit history read,
- withdrawal history read,
- transfer history read,
- addresses read where supported.

Фокус:
- read-only foundation before write operations.

### 35.4 `0.4.x+` — Asset write / convert (opt-in)

Добавить только при зрелости:
- internal transfers,
- convert,
- controlled asset write APIs,
- dangerous operations behind explicit gates.

### 35.5 `1.0` — Stable unified futures-first engine

К моменту `1.0` должны быть стабильны:
- futures unified API,
- instrument model,
- health model,
- state/reconcile semantics,
- error taxonomy,
- capability model.

---

## 36. Что делать нельзя

Чтобы проект не деградировал в сложность и костыли, запрещаются следующие анти-паттерны:

1. Один giant `Exchange` trait на всё.
2. Унификация через `serde_json::Value` как основной контракт.
3. Стабилизация низкоуровневого adapter trait слишком рано.
4. Встраивание БД в core.
5. Магические retry policies без classification.
6. Прятать `UnknownExecution` под generic timeout.
7. Утечка exchange-specific полей в базовые типы без отдельного native слоя.
8. Перенос UI/business logic в библиотеку.
9. Heavy normalization прямо в socket hot path.
10. Преждевременный codegen-driven design.

---

## 37. Definition of Done для `0.1.0`

`0.1.0` считается успешной, если:

1. Binance и Bybit linear futures работают через единый facade.
2. Public streams стабильны на длительном прогоне.
3. Private streams корректно поддерживают состояние orders/positions/balances.
4. Reconnect + reconcile покрыты тестами и не являются “best effort only”.
5. Error taxonomy usable для реальных приложений.
6. Capability matrix задокументирован.
7. Есть понятные examples:
   - ticker stream,
   - order create/cancel,
   - positions,
   - native access.
8. Нет встроенной DB, нет hidden globals, нет giant dynamic models.
9. Публичный API маленький и читаемый.
10. Документация объясняет не только “как использовать”, но и “что не поддерживается”.

---

## 38. Итоговое решение

Итоговая форма проекта:

> **`bat-markets` — это futures-first open-source Rust exchange kernel с тремя API-слоями, тремя execution lanes, двумя адаптерами и одним стабильным facade crate.**

Это ядро должно быть:

- быстрым для market data,
- строгим для private trading state,
- честным в унификации,
- маленьким по публичной поверхности,
- модульным по внутренней архитектуре,
- удобным для open-source сопровождения.

---

## 39. Краткий архитектурный манифест

Если сформулировать совсем коротко, то `bat-markets` должен следовать таким правилам:

- **не врать про одинаковость бирж;**
- **не тащить всё в один слой;**
- **не строить hot path на удобстве;**
- **не скрывать uncertain state;**
- **не замораживать внутренности раньше времени;**
- **не смешивать ядро и продуктовые workflow;**
- **делать меньше, но делать это правильно.**

---

## 40. Следующий шаг после этого документа

После принятия этого blueprint нужно сделать только три вещи, в таком порядке:

1. зафиксировать `ADR-0001` с этой формой проекта;
2. создать workspace и пустые crates именно в этой структуре;
3. реализовывать **только `0.1.0` futures-first scope**, не добавляя asset/spot/write фичи раньше времени.

---
