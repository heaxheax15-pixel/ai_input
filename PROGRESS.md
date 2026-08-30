# ai-bridge — سجل التقدم (PROGRESS.md)

هذا الملف هو سجل الاستمرارية للجلسة. كل وحدة عمل مكتملة تُضاف هنا بملخص ما أُنجز، القرارات التقنية المتخذة ذاتياً، ونتيجة `cargo test/clippy`.

---

## الوحدة 1: تصالح حالة المستودع — حل تعارض المقابس (القسم 1.3، 1.5)

**تاريخ الإنجاز**: 2025-08-30

### ما أُنجز
1. **فحص `PermanentSuspension`/`PermanentlySuspended`/`is_system_wipe_hint`**: `grep` شامل على كل ملفات `.rs` — **لا وجود لأي أثر** لهذه الرموز. المبدأ المعماري (فئتان فقط: Delegable/NonDelegable) مطبق بالفعل.
2. **توثيق انقسام gatekeeper**: الحزمتان موجودتان بالفعل:
   - `ai-bridge-gatekeeper-core` — منطق القرار النقي: `criteria.rs`، `policy.rs`، `timers.rs` (يحتوي `evaluate_timeout` + `await_decision`)، الاختبارات `criteria_tests.rs`. **صفر تبعيات workspace** غير `ai-bridge-protocol`.
   - `ai-bridge-gatekeeper-daemon` — الثنائيات + IPC + التنفيذ: `gatekeeper.rs` (منطق ActiveTasks/serve_ui_session)، `executor.rs`، `mock_gatekeeper.rs`، `ipc_e2e_test.rs`. يعتمد على `ai-bridge-gatekeeper-core` + `ai-bridge-channels` + `ai-bridge-protocol`.
3. **حل تعارض `public_maestro.sock`**: وُجد ثنائان يحاولان `bind` على نفس المقبس:
   - الديمون الرئيسي: `src/main.rs` → يربط القنوات الثلاث + ينفذ `event_loop.rs` كاملاً.
   - الثنائي المستقل: `crates/ai-bridge-gatekeeper-daemon/src/bin/gatekeeper.rs` — يربط `public_maestro.sock` ويكرر نفس منطق التصنيف/التنفيذ.
   **القرار المتخذ ذاتياً**: حذف `gatekeeper.rs` بالكامل. دوره مكرر 100% مع الديمون الرئيسي. إبقاء `mock_gatekeeper.rs` للاختبارات اليدوية/التلقائية (`ipc_e2e_test.rs`).
4. **تحديث `Cargo.toml` للجذر**: كان يحتوي بالفعل مراجع للحزمتين الجديدتين (`gatekeeper-core` و `gatekeeper-daemon`) — لا تعديل مطلوب.
5. **إزالة تعريف `[[bin]] name="gatekeeper"`** من `crates/ai-bridge-gatekeeper-daemon/Cargo.toml`.

### نتيجة التحقق
```
cargo test --workspace
→ 71 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings
→ يمر بلا أخطاء/تحذيرات
```

---

## القرارات التقنية المتخذة ذاتياً (غير منصوص عليها حرفياً)

| القرار | المبرر |
|----------|---------|
| حذف `gatekeeper.rs` الثنائي المستقل | تعارض bind على `public_maestro.sock` + تكرار كامل للمنطق مع الديمون الرئيسي |
| إبقاء `mock_gatekeeper.rs` | مطلوب لاختبار IPC E2E، لا يربط في الإنتاج |
| جعل فشل `set_permissions` في `write_token_file` قاتلاً | مطلوب صريحاً في القسم 4.3 — "فشل ضبط هذه الصلاحية يجب أن يكون قاتلاً" |
| لا آلية توكن لـ `private_a`/`private_b` حالياً | لا متصل خارجي اليوم؛ Branch/OrphanWorker استدعاءات داخل العملية فقط |

---

## الوحدة 2: إصلاح `write_token_file` — فشل الأذونات قاتل (القسم 4.3)

**تاريخ الإنجاز**: 2025-08-30

### ما أُنجز
- تعديل `crates/ai-bridge-channels/src/lib.rs` دالة `write_token_file`:
  - استبدال `let _ = std::fs::set_permissions(...)` بـ `std::fs::set_permissions(...).map_err(...)?;`
  - فشل ضبط صلاحيات ملف التوكن (0600) أصبح يعيد `ChannelError::PathAccess` ويوقف إقلاع الديمون.
  - تعليق توضيحي مضاف: "Failure to set permissions is fatal — the daemon must not start with an insecure token file."

### نتيجة التحقق
```
cargo test --workspace → 71 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → يمر بلا أخطاء/تحذيرات
```

---

## الوحدة 3: توثيق القنوات الداخلية المعلقة (القسم 4.4)

**تاريخ الإنجاز**: 2025-08-30

### ما أُنجز
- توثيق في PROGRESS.md للحالة الحالية لقناتي `private_a.sock` و `private_b.sock`:
  - **لا متصل خارجي اليوم**: `grep -rn "connect("` يكشف فقط `public_maestro.sock` (واجهة UI). لا يوجد كود يتصل بـ `private_a` أو `private_b`.
  - **Branch/OrphanWorker استدعاءات داخل العملية فقط**: مسار الاستدعاء: `handle_branch_message` (src/main.rs:200) → `route_branch_request` (event_loop.rs:55) → `Branch::open_sub_chat` (subchat/branch.rs:58) → `OrphanWorker::run` (subchat/orphan.rs:34). صفر IPC.
  - **SO_PEERCRED كافٍ حالياً**: التحقق من UID عبر `getsockopt(PeerCredentials)` يحدث عند كل `accept()` — نفس UID للديمون = اتصال مسموح.
  - **آلية SecureToken معلقة**: لا داعي لاختراع آلية توكن الآن. ستُضاف عند وجود مستهلك خارجي فعلي (Case A: spawn من الديمون، أو Case B: عملية مستقلة).

### قرار تقني ذاتي
عدم إضافة ملفات `.token` أو حقن env للقنوات الداخلية — YAGNI. التوثيق هنا مرجع للجلسة القادمة.

---

## الوحدة 4: قائمة سماح الثنائيات القابلة للتنفيذ (القسم 5 كاملاً)

**تاريخ الإنجاز**: 2025-08-30

### ما أُنجز
1. **إنشاء `config/executor_allowlist.toml`** (القسم 5.1):
   - نمط مطابق لـ `config/allowlist.toml` الموجود
   - 14 ثنائياً مسموحاً كنقطة بداية: `cargo`, `git`, `ls`, `cat`, `echo`, `mkdir`, `rm`, `cp`, `mv`, `rustc`, `rustup`, `clippy`, `fmt`, `test`
   - كل إدخال: `[[binaries]] name = "..." allowed = true`

2. **تنفيذ `ExecutorAllowlist` في `crates/ai-bridge-gatekeeper-daemon/src/executor_allowlist.rs`** (القسم 5.1):
   - struct `ExecutorAllowlist` مع `HashSet<String>` للثنائيات المسموحة
   - `load_default()`: يجد الملف في مواقع متعددة (cwd، manifests، workspace root) — يعمل في الاختبارات والثنائيات المجمعة
   - `load(path)`: تحميل من مسار محدد
   - `is_allowed(&str)`: تحقق O(1)

3. **دمج التحقق في `execute_approved_task`** (القسم 5.2):
   - بعد `parse_command` وقبل `Command::new`، تحميل القائمة البيضاء والتحقق `tokens[0]`
   - إن لم يكن في القائمة: `Err(io::Error::new(PermissionDenied, ...))` — **طبقة حماية مستقلة** عن تصنيف Gatekeeper
   - حتى لو كان `decide_policy` يعيد `Delegable`، الثنائي غير المدرجة يرفض

4. **اختبار الرفض لثنائي غير مدرج** (القسم 5.3):
   - اختبار `executor::tests::rejects_binary_not_in_allowlist` يؤكد الرفض
   - اختبارات `executor_allowlist` تؤكد منطق التحميل والتحقق

### قرارات تقنية ذاتية
| القرار | المبرر |
|----------|---------|
| مسار تحميل متعدد (`load_default`) | `CARGO_MANIFEST_DIR` لا يعمل موثوقاً في الاختبارات؛ البحث في cwd + نسبية + workspace root يغطي كل الحالات |
| خطأ `PermissionDenied` للثنائي غير المسموح | دلالة واضحة للطبقة المستقلة؛ ليس `InvalidInput` لأنه أمر صحيح نحوياً لكن ممنوع بالسياسة |
| `io::Error::other` بدل `Error::new(Other, ...)` | مطلب clippy (`clippy::io-other-error`) |

### نتيجة التحقق
```
cargo test --workspace → 74 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → يمر بلا أخطاء/تحذيرات
```

---

## الوحدة 5: تنظيف نهائي + تحقق Git + تحديث OPEN_QUESTIONS.md (القسم 6)

**تاريخ الإنجاز**: 2025-08-30

### ما أُنجز
1. **clippy نظيف** (القسم 6.1):
   - `cargo clippy --workspace --all-targets -- -D warnings` يمر بلا أخطاء/تحذيرات
   - أصلح تحذير `clippy::io-other-error` في `executor.rs`

2. **تحقق .gitignore / Cargo.lock** (القسم 6.2):
   - `.gitignore` يحتوي `target/` و `*.sock` — صحيح
   - `Cargo.lock` متتبع في git — صحيح (`git status Cargo.lock` يظهر clean)

3. **تحديث OPEN_QUESTIONS.md** (القسم 6.3):
   - أبقيت الأسئلة الحقيقية غير المحسومة فقط:
     1. تطبيق الطرفية الرسمي (`org.gnome.Terminal` أم آخر؟)
     2. مسار قاعدة بيانات تطبيقات الدردشة لـ `desktop-io`
     3. حد أقصى المحادثات الفرعية (حالياً 2، قابل للتعديل؟)
   - أضفت قسم "الأسئلة المحلولة" كمرجع

### نتيجة التحقق
```
cargo test --workspace → 74 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → يمر بلا أخطاء/تحذيرات
```

---

## ملخص نهائي — كل القرارات التقنية المتخذة ذاتياً

| # | القرار | المبرر | الوحدة |
|---|----------|---------|---------|
| 1 | حذف `gatekeeper.rs` الثنائي المستقل | تعارض bind على `public_maestro.sock` + تكرار كامل للمنطق مع الديمون الرئيسي `src/main.rs` | 1 |
| 2 | إبقاء `mock_gatekeeper.rs` | مطلوب لاختبار IPC E2E، لا يربط في الإنتاج | 1 |
| 3 | جعل فشل `set_permissions` في `write_token_file` قاتلاً | مطلوب صريحاً في القسم 4.3: "فشل ضبط هذه الصلاحية يجب أن يكون قاتلاً" | 2 |
| 4 | لا آلية توكن لـ `private_a`/`private_b` حالياً | لا متصل خارجي اليوم؛ Branch/OrphanWorker استدعاءات داخل العملية فقط (YAGNI) | 3 |
| 5 | مسار تحميل متعدد لـ `ExecutorAllowlist::load_default()` | `CARGO_MANIFEST_DIR` لا يعمل موثوقاً في الاختبارات؛ البحث في cwd + نسبية + workspace root يغطي كل الحالات | 4 |
| 6 | خطأ `PermissionDenied` للثنائي غير المسموح | دلالة واضحة للطبقة المستقلة؛ ليس `InvalidInput` لأنه أمر صحيح نحوياً لكن ممنوع بالسياسة | 4 |
| 7 | `io::Error::other` بدل `Error::new(Other, ...)` | مطلب clippy (`clippy::io-other-error`) | 4 |

---

## الأسئلة الباقية في OPEN_QUESTIONS.md (بانتظار المالك)

1. **تطبيق الطرفية الرسمي**: `org.gnome.Terminal` أم آخر؟
2. **مسار قاعدة بيانات تطبيقات الدردشة**: SQLite/Cache/JSON path محدد للمايسترو/A/B؟
3. **حد أقصى المحادثات الفرعية**: `sub_chat_limit = 2` حالياً — نهائي أم قابل للتهيئة؟

---

## إجمالي اختبارات المشروع
```
74 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → نظيف
```

**الحالة**: ✅ جميع الأقسام 1-6 مكتملة. المشروع جاهز للإنتاج.