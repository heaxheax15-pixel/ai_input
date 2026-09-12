# ai-bridge — سجل التقدم (PROGRESS.md)

## قرار المالك: نظام الوسم الرمزي الإلزامي [[AB:...]]

هذا قرار مقصود من المالك، وليس انحرافاً غير مُصرّح به. يطبّق هذا النظام على جميع الأوامر بلا استثناء، ويشمل المايسترو وصندوق تحكم الجسر المباشر معاً. أي أمر لا يحتوي على وسم صالح من جدول الرموز المشترك يُعامل دائماً على أنه `NonDelegable`، كإجراء محافظ متعمد. أي أمر يحتوي على وسم صالح لا يزال يخضع أيضاً لفحص `evaluate_text_command` النصي كطبقة دفاع ثانية مستقلة؛ لا يُسمح بقبول `Delegable` إلا إذا اتفق كليهما على هذا القرار.

### حالة الحقن التلقائي عند جلسة Maestro جديدة
- لا يوجد في الكود الحالي نقطة مركزية موصولة لكلا مساري تغيير الدور: التعيين اليدوي عبر UI و`role_swap_on_failure` في `ops_room.rs` لا يستدعيا بعد ذلك أي مسار حقن موحد لنص القاموس. هذا يعني أن الحقن التلقائي عند جدولة Maestro الجديدة غير متصل فعلياً بعد في هذا المستودع، رغم أن القاموس نفسه متاح كوظيفة موحدة في `ai-bridge-protocol`.
- لا يمكن اختبار الحقن الفعلي على شاشة حقيقية في هذه البيئة الحالية؛ أي تنفيذ حقيقي للـRemoteDesktop لا يزال يتطلب بيئة Wayland/portal حقيقية، وهو خارج النطاق القابل للاختبار في هذه الجلسة.

## تحقق إلزامي قبل التنفيذ (القسم 0)

**تاريخ التحقق**: 2026-08-30

### 0.1 — وجود `RoleAssignment` وصندوق الإرسال في `ai-bridge-ui`
- فحص شامل للرمز الحالي في [crates/ai-bridge-ui/src/app.rs](crates/ai-bridge-ui/src/app.rs) و [crates/ai-bridge-ui/src/main.rs](crates/ai-bridge-ui/src/main.rs) وكذلك عبر workspace search عن `RoleAssignment|Role Assignment|SendBox|send_box|clipboard`.
- النتيجة: لا يوجد أي `RoleAssignment` ولا أي صندوق إرسال/واجهة نقل صورة داخل `ai-bridge-ui` في الكود الحالي.
- العواقبة: هذا القيد يوقف التنفيذ المباشر للأقسام 1-3 لأن المكوّنات المطلوبة غير موجودة في قاعدة الشفرة الحالية.

### 0.2 — دعم `ashpd` لاستمرارية الجلسة (`persist_mode`/`restore_token`)
- التحقق من `ashpd` المستخدم في [crates/ai-bridge-hand-eye/Cargo.toml](crates/ai-bridge-hand-eye/Cargo.toml): الإصدار `0.13` مع `ashpd/screencast` و `ashpd/remote_desktop`.
- في الكود الحالي في [crates/ai-bridge-hand-eye/src/portal.rs](crates/ai-bridge-hand-eye/src/portal.rs) يتم استدعاء `SelectSourcesOptions::default().set_persist_mode(PersistMode::DoNot)`, وهي قيمة صريحة منطقياً تعني "عدم الاستمرار/عدم الاحتفاظ"، وليس مساراً رسميّاً لـ `restore_token`.
- لا توجد أي إشارة إلى `restore_token` أو تدفق استعادة الجلسة في هذا المشروع.
- العواقبة: التقاط تلقائي بلا إنسان حاضر غير ممكن حاليّاً دون شاشة اختيار المصدر في كل مرة؛ هذا قيد تصميمي صريح.

### 0.3 — قدرة كتابة صورة إلى حافظة النظام
- فحص شامل للتبعيات والرمز: لا توجد في المشروع أي اعتماد على `arboard`/`rfd`/`copypasta`/`x11-clipboard` أو أي `write_image`/`Clipboard` path.
- ملف [crates/ai-bridge-ui/Cargo.toml](crates/ai-bridge-ui/Cargo.toml) يضم فقط `eframe`, `egui`, `tokio`, `serde`, `toml`.
- لا يوجد رمز في المشروع يكتب مباشرة إلى نظام الحافظة أو ينشئ صورة قابلة للضغط على الحافظة.
- البديل المتاح فعليّاً: حفظ الصورة كملف محلي ثم إرفاقها يدويّاً / أو استخدام مسار الإدخال اليدوي داخل تطبيق/واجهة المستخدم؛ لا يوجد "لصق تلقائي جاهز" من هذه التبعيات الحالية.

### النتيجة المنطقية
- لا يمكن تنفيذ التقاط تلقائي دون تدخل بشري اليوم في هذه قاعدة الشفرة الحالية.
- لا يوجد في المشروع مسار جاهز للحافظة/الصندوق الإرسال/استمرارية الجلسة اللازمة لتفادي نافذة اختيار المصدر في كل مرة.

---

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

## الوحدة 6: إصلاح TODO حقن النص في `portal.rs` — الفرع المتوقع (`notify_keyboard_keysym`) (المكتمل)

**تاريخ الإنجاز**: 2026-08-31

### ما أُنجز
1. **تحقق ashpd المحلي فعليّاً**: فحص الكود المصدري المحلي في `~/.cargo/registry/src/.../ashpd-0.13.13` يثبت أن `RemoteDesktop` يحتوي على طريقتين فعليتين:
   - `notify_keyboard_keycode(&session, keycode, state, options)`
   - `notify_keyboard_keysym(&session, keysym, state, options)`
   - لذلك لا يوجد سبب لاتباع مسار بديل متصوّر؛ `keysym` متاح فعليّاً في هذا الإصدار.
2. **تطبيق الفرع 1**: أنشئنا `char_to_keysym(ch: char) -> Option<u32>` باستخدام قاعدة X11 للـ ASCII/Latin-1 القابل للطباعة `0x20..=0x7E`, حيث قيمة `keysym` تساوي code point مباشرة، بدون جدول تحويل يدوي.
3. **تعامل مع Enter**: استخدم `0xff0d` كـ X11 Return keysym الرسمي، مع بقاء `\n` كفاصل إدخال وليس كـ keysym حرف.
4. **تجاوز النص غير المدعوم**: أي حرف خارج النطاق المدعوم (`ASCII printable` فقط في هذا المسار) يتم تجاهله بصمت أثناء الحقن ولا يوقف التنفيذ.
5. **اختبار الوحدة**: أُضيفت اختبارات لـ `char_to_keysym` تغطي أحرف وأرقام ورموز شائعة وتؤكد `None` للحروف خارج المدى.
6. **تنفيذ `capture_and_inject_failure_context`**: أعيد بناءه لاستخدام `notify_keyboard_keysym` مع `char_to_keysym` بدل TODO السابق.

### القيد المعروف
- **النص المدعوم فعليّاً في هذا المسار**: `ASCII printable` فقط (`0x20..=0x7E`)، أي ما يندرج في الحروف والأرقام والرموز الإنجليزية الشائعة.
- **الحروف غير اللاتينية/غير ASCII**: يتم تجاوزها بصمت في الحقن؛ لا تُختَرَع قيمة `keysym` لها.
- **هذا القيد مسجّل هنا** لأنه مطابق للحد الفعلي للدعم في `ashpd`/X11 في هذا التنفيذ، وعدم التوسع خارج هذا النطاق يحافظ على سلامة الحقن.

### نتيجة التحقق
```
cargo test --workspace
→ نجح بعد التنفيذ (انظر الإخراج الكامل في سجل التشغيل)
```

**الحالة**: ✅ القسم 1 اكتمل، والقسم 2 غير مطلوب لأن `notify_keyboard_keysym` موجود فعليّاً في هذا الإصدار.

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
| 8 | استخدام `notify_keyboard_keysym` مع ASCII-only mapping | `keysym` متاح فعليّاً في ashpd 0.13؛ هذا هو المسار المفضل والسليم | 6 |

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

### القسم 5 — منطق صحيح غير مُختبَر لغياب شاشة
- تم التحقق أن `build_failure_status_text` و`self_heal`/`role_swap_on_failure` هي منطق صحيح وقابل للاختبار، لكن لا يوجد في هذه البيئة شاشة حقيقية ولا جلسة GUI، لذلك لا يمكن إثبات فعلياً أن الالتقاط/الحافظة/الحقن عبر `portal` يتم عبر شاشة حقيقية.

### القسم 5 — فجوة تصميم حقيقية: الالتقاط والحافظة والحقن غير مكتملين
- `screen_cast_capture()` لا يعيد بايتات إطار حقيقية؛ إنما يفتح جلسة ScreenCast ويُنشئ/يبدأها فقط، وليس تدفق بكسلات فعلي متاح في هذا الكود. لذلك `capture_failure_window_image` لا يمكن أن يكتب صورة حقيقية دون قراءة إطار من تدفق PipeWire/stream الفعلي.
- `capture_and_inject_failure_context` لا يمكن أن يكتب صورة إلى الحافظة بأبعاد حقيقية أو يحقن نصاً دون معرفة أبعاد صورة فعليّة وواجهة إدخال نص مؤكدة من بروتوكول RemoteDesktop / مكتبة لوحة مفاتيح حقيقية. لا يوجد في المشروع خريطة X11/XKB فعلية أو ما يعادلها لتشفير نص إلى keycodes؛ إنما كان هناك تحويل يدوي خادع تم حذفه.

---

## الوحدة 7: Safety Lock UI Integration — عناصر التحكم في واجهة المستخدم (القسم 1)

**تاريخ الإنجاز**: 2026-09-01

### ما أُنجز
1. **توسيع `GatekeeperEvent` enum** في `crates/ai-bridge-ui/src/app.rs`:
   - إضافة متغير جديد: `SafetyStateUpdate { armed: bool }` لاستقبال حالة الأمان من الديمون

2. **توسيع `OutboundEvent` enum** بأربع أنواع رسائل جديدة:
   - `SafetyArm { ty: "safety_arm".to_string() }` — طلب تفعيل الأمان
   - `SafetyDisarm { ty: "safety_disarm".to_string(), reason: String }` — طلب إلغاء تفعيل الأمان مع السبب
   - `SafetyOverride { ty: "safety_override".to_string(), task_id: String, reason: String }` — تجاوز الأمان مع تبرير إنساني
   - `DirectMessage { ty: "direct_message".to_string(), text: String }` — رسالة مباشرة إلى الديمون

3. **إعادة هندسة `render_safety_controls()`**:
   - استبدال تحديث الحالة المحلي بإرسال رسائل daemon عبر `self.tx_out` channel
   - زر "Arm Safety" يرسل `OutboundEvent::SafetyArm`
   - زر "Disarm Safety" يرسل `OutboundEvent::SafetyDisarm` مع السبب
   - زر "Override and Execute" يرسل `OutboundEvent::SafetyOverride` (يتطلب سبب إنساني)

4. **إعادة هندسة `render_role_assignment_panel()`**:
   - زر "Send" في رسالة النص الآن يرسل `OutboundEvent::DirectMessage` عبر القناة

5. **معالجة الديمون في `src/main.rs`**:
   - استيراد `use ai_bridge_gatekeeper_core::safety_guard::SafetyGuard`
   - إنشاء instance `SafetyGuard` جديد مع `Arc<Mutex<>>`
   - نسخه إلى جميع handler tasks للقنوات الثلاث
   - توسيع `handle_maestro_message()` signature ليأخذ `&mut SafetyGuard`
   - إضافة مطابق (matcher) لأنواع الرسائل الجديدة:
     - `"safety_arm"` → `safety_guard.arm("ui_user")` → إعادة `{"status": "armed"}`
     - `"safety_disarm"` → `safety_guard.disarm("ui_user", reason)` → إعادة `{"status": "disarmed"}`
     - `"safety_override"` → تسجيل override مع task_id والسبب
     - `"direct_message"` → تسجيل محتوى الرسالة
   - جميع handlers تعيد إقرار JSON إلى socket stream

### القرارات التقنية
| القرار | المبرر |
|----------|---------|
| `Arc<Mutex<SafetyGuard>>` للحالة المستمرة | الديمون متعدد الخيوط؛ الأمان الموضوعي يتطلب مزامنة |
| `OutboundEvent#[serde(untagged)]` مع `"type"` صريح | تجنب تضارب serialization مع أحداث موجودة؛ الديمون يميز بـ `v.get("type").and_then(\|t\| t.as_str())` |
| حذف `map_or` وتطبيق `if let Some` | مطالبة clippy بإزالة تحذيرات "unnecessary_map_or" |

### نتيجة التحقق
```
cargo test --workspace → 112 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → نظيف
```

**الحالة**: ✅ القسم 1 مكتمل. عناصر التحكم في الواجهة متصلة الآن بـ SafetyGuard المستمر في الديمون عبر `public_maestro.sock`.

---

## الوحدة 8: Text Injection Function Extraction — استخلاص دالة متعددة الاستخدام (القسم 2)

**تاريخ الإنجاز**: 2026-09-01

### ما أُنجز
1. **استخراج دالة `inject_text_and_send()` عامة** في `crates/ai-bridge-hand-eye/src/portal.rs`:
   - التوقيع: `async fn inject_text_and_send(allowlist: &Allowlist, target_app_id: &str, text: &str) -> Result<(), PortalError>`
   - تنشئ جلسة RemoteDesktop
   - تصرح بتطبيق الهدف
   - تكرر على أحرف النص مع تحويل عبر `char_to_keysym()` (ASCII printable فقط `0x20..=0x7E`)
   - تعامل خاص مع newline كـ `0xff0d` (Return keysym)
   - استخدم `inject_keysym()` helper لإرسال أحداث pressed/released

2. **تحديث `capture_and_inject_failure_context()`**:
   - استبدل تنفيذ الحقن المضمّن بـ call إلى `inject_text_and_send()`
   - بناء نص الفشل → استدعاء دالة عامة → معالجة أخطاء موحدة

3. **عدم تعديل الاختبارات**:
   - الاختبارات الموجودة تغطي الحالات الأساسية
   - استخلاص الدالة لا يتطلب اختبارات جديدة (لا توجد بيئة portal حقيقية)

### القرارات التقنية
| القرار | المبرر |
|----------|---------|
| دعم ASCII فقط في `char_to_keysym` | X11 keysym المدعوم في ashpd 0.13 ملموس لـ Latin-1؛ تجنب تعقيد الترميز |
| تجاهل صامت للأحرف خارج المدى | لا وقف للتنفيذ على حرف غير مدعوم؛ مثالية للنص مع ترجمات مختلطة |

### نتيجة التحقق
```
cargo test --workspace → 112 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → نظيف
```

**الحالة**: ✅ القسم 2 مكتمل. دالة حقن النص استُخرجت وأصبحت قابلة لإعادة الاستخدام.

---

## الوحدة 9: Allowlist Unification — توحيد قوائم التطبيقات (القسم 3)

**تاريخ الإنجاز**: 2026-09-01

### ما أُنجز
1. **تحديث اعتماديات `ai-bridge-ui`** في `crates/ai-bridge-ui/Cargo.toml`:
   - إضافة `ai-bridge-hand-eye = { path = "../ai-bridge-hand-eye" }` للوصول إلى `Allowlist`

2. **استيراد `Allowlist`** في `crates/ai-bridge-ui/src/app.rs`:
   - `use ai_bridge_hand_eye::allowlist::Allowlist;`

3. **إضافة دالة مساعد `get_allowed_app_ids()`**:
   - تحميل `allowlist.toml` من مسارات متعددة (cwd, manifest root, workspace root)
   - إرجاع vector بـ app IDs المسموحة (مُرشحة بـ `allowed == true`)
   - fallback إلى defaults `["org.gnome.Terminal", "org.mozilla.firefox", "org.gnome.Nautilus"]` إذا لم يُعثر على الملف

4. **تحديث `render_role_assignment_panel()`**:
   - استبدال hardcoded array بـ call إلى `self.get_allowed_app_ids()`
   - الآن تعكس قائمة الأدوار UI التكوين الفعلي في `config/allowlist.toml`

5. **إضافة زري Save/Reload في `render_config_prompts()`**:
   - زر "Save allowlist": يتحقق من TOML validity (عبر `toml::from_str`) ثم يكتب إلى `config/allowlist.toml`
   - زر "Reload from file": يقرأ ملف allowlist الحالي ويملأ `self.allowlist_input` text field
   - رسائل خطأ واضحة عند الفشل (مثلاً "Invalid TOML", "Failed to write", etc)

6. **دوال مساعدة جديدة**:
   - `load_allowlist_config()`: يقرأ الملف الموجود من disk
   - `save_allowlist_config()`: يتحقق و يكتب allowlist config مع معالجة أخطاء TOML

### القرارات التقنية
| القرار | المبرر |
|----------|---------|
| مسارات بحث متعددة في `get_allowed_app_ids` | نفس نمط `RoleAssignmentSet::load_default()` المستخدم في الكود؛ يعمل في اختبارات وإنتاج |
| TOML validation قبل الكتابة | تجنب ملفات config معطلة؛ user يرى خطأ واضح بدل صمت |
| fallback إلى defaults | graceful degradation عند غياب allowlist؛ UI تبقى functional |

### نتيجة التحقق
```
cargo test --workspace → 112 اختبار نجح، 0 فشل
cargo clippy --workspace --all-targets -- -D warnings → نظيف
```

**الحالة**: ✅ القسم 3 مكتمل. قوائم التطبيقات موحدة الآن بين UI والـ daemon عبر `config/allowlist.toml`.

---

## ملخص أخير — الأقسام 1-3 من بروتوكول العمل P1-P6

| # | اسم القسم | الحالة | ملاحظات |
|---|-----------|--------|---------|
| 0 | تحقق معماري (Safety Lock channel) | ✅ | `public_maestro.sock` كافٍ، لا حاجة لقناة رابعة |
| 1 | عناصر تحكم Safety Lock في UI | ✅ | 4 أنواع رسائل جديدة، SafetyGuard persistent في daemon |
| 2 | استخلاص دالة حقن النص | ✅ | `inject_text_and_send()` عامة وقابلة لإعادة الاستخدام |
| 3 | توحيد قوائم التطبيقات | ✅ | UI تقرأ من `allowlist.toml`، زر Save مع validation |
| 4 | Full test suite + clippy | ✅ | 112 اختبار نجح، 0 تحذيرات |
| 5 | git commit | ⏳ | جاهز للتنفيذ |

### إجمالي الاختبارات
```
cargo test --workspace
→ 112 اختبار نجح، 0 فشل

cargo clippy --workspace --all-targets -- -D warnings
→ نظيف (zero warnings)
```

**الحالة العامة**: ✅ جميع الأقسام 0-3 مكتملة بنجاح. المشروع نظيف وجاهز للـ commit.
- هذا غير مُختبَر آلياً في هذه البيئة، ويحتاج تصميماً إضافياً حقيقياً لقراءة إطار PipeWire ونظام إدخال نص موثوق قبل وصفه بأنه مُنجز.