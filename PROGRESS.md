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

## النقاط المعلقة (ستعالج في الوحدات اللاحقة)

- [ ] **الوحدة 4**: قائمة سماح الثنائيات القابلة للتنفيذ (القسم 5 كاملاً).
- [ ] **الوحدة 5**: تنظيف نهائي + تحقق Git + تحديث OPEN_QUESTIONS.md (القسم 6).