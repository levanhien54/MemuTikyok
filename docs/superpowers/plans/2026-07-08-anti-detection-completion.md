# Anti-detection Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Đóng các tell "sửa được mà chưa sửa" và hở nhất quán của lớp chống-phát-hiện MuMu-x86 (build.characteristics, SoC/GPU/patch coherent, IMEI-TAC, bộ device, android_id qua restore, tự-soi cuối provision) mà không phá các bất biến vòng đời đã có.

**Architecture:** Mở rộng `HardwareProfile`/`DeviceProfile` bằng các trường coherent (nguồn sự thật per-account), tách phần sinh script resetprop thành hàm THUẦN để unit-test được, và set thêm prop qua resetprop runtime (đường đã hoạt động). Bảo toàn android_id per-app qua backup/restore `settings_ssaid.xml`. Mở rộng `FingerprintLockStatus` (#26) thành `ProvisionHealth` tự soi dấu vết cuối provision. Mọi prop **bỏ qua nếu giá trị rỗng** (nguyên tắc #07: thà không set còn hơn set mâu thuẫn).

**Tech Stack:** Rust (Tauri 2 backend), `serde` camelCase, `rusqlite`, `async-trait`, resetprop (Magisk applet), MuMuManager CLI; React/TypeScript frontend (types trong `src/types/instance.ts`).

## Global Constraints

- **Không bịa dữ liệu fingerprint thiết bị.** TAC, `ro.hardware`, `ro.board.platform`, `security_patch` phải là giá trị THẬT của model đó (nguồn: getprop máy thật, hoặc DB fingerprint đã kiểm như `u0pattern/Anti-EmuDetector`). Trường chưa xác minh để **rỗng** → code bỏ qua (không set), không set giá trị đoán.
- **Mô hình VM disposable — không reboot.** Mọi thay đổi phải ăn ở **runtime** (resetprop), không dựa `sed /system/build.prop` + reboot.
- **Giữ FE/BE type parity.** Thêm field vào `model.rs::HardwareProfile` phải phản ánh sang `src/types/instance.ts` (camelCase, optional) và `src/lib/mockBackend.ts::mockFingerprint`.
- **Back-compat serde.** Field mới trong struct persist phải `#[serde(default)]` để hồ sơ cũ trong SQLite vẫn nạp được.
- **Cổng chất lượng sau mỗi task:** `cargo test --lib` xanh, `cargo clippy --all-targets` 0 warning, và (nếu chạm FE) `npm run typecheck && npm run lint && npm run test` xanh.
- **Convention:** tên test tiếng Việt không dấu/snake_case như các test hiện có; prop set qua hàm thuần `build_lock_script` để test không cần MuMu.
- **Không đụng automation (human_tap/human_swipe)** — ngoài phạm vi (scope memory: NO automation development).
- **Cổng coherence cuối cùng là A.4 trên MuMu thật:** `cargo test --lib e2e_real::a4 -- --ignored --nocapture` (đọc lại getprop, xác nhận khớp profile). Unit test kiểm cơ chế; A.4 kiểm giá trị thật.

---

## File Structure

- `src-tauri/src/model.rs` — `HardwareProfile` +5 field coherent (`soc_hardware`, `board_platform`, `gpu_egl`, `security_patch`, `build_characteristics`). Nguồn dữ liệu bền per-account.
- `src-tauri/src/fingerprint.rs` — `DeviceProfile` +6 field (5 trên + `tac`); bảng `DEVICES` điền giá trị thật; `gen_imei(tac)`, `pick_device_index` (rejection sampling), `generate()` map field mới; unit test.
- `src-tauri/src/adb.rs` — tách `build_lock_script(rp, hw) -> String` (thuần, testable) khỏi `lock_device_identity`; thêm các prop coherent + xử lý `ro.build.characteristics`; **gỡ** khối `sed`/remount chết trong `harden()`; unit test cho script.
- `src-tauri/src/e2e_real.rs` — mở rộng A.4 assert getprop mới (ignored, chạy trên MuMu thật).
- `src/types/instance.ts` — `HardwareProfile` +5 field optional (camelCase).
- `src/lib/mockBackend.ts` — `mockFingerprint` trả field mới (cho UI demo).
- **Phase B:** `src-tauri/src/adb.rs` (backup/restore SSAID), `e2e_real.rs` (test round-trip android_id).
- **Phase C:** `src-tauri/src/model.rs` (`ProvisionHealth`), `orchestrator.rs`, `profile_ops.rs`, `commands.rs`, FE `instance.ts`/`useProfileStore.ts`/`ProfilesView.tsx`.

---

## Progress (cập nhật 2026-07-08)

**Cổng chất lượng:** Rust `cargo test --lib` 66 pass / 0 fail · `cargo clippy` 0 warning · FE typecheck + lint + 13 test — tất cả xanh.

| Task | Trạng thái | Ghi chú |
| :-- | :-- | :-- |
| **A1** field coherent + FE parity + mock | ✅ Xong | `HardwareProfile` +5 field, `instance.ts` + `mockFingerprint` khớp |
| **A2** `build_lock_script` + prop coherent + gỡ sed | ✅ Xong | có unit test; verify thêm `ro.build.characteristics != tablet` |
| **A3** IMEI ghép TAC (fallback random) | ✅ Xong | test `imei_dung_tac`, `imei_random` |
| **A4** rejection sampling chọn device | ✅ Xong | test `chon_device_khong_lech_modulo` |
| **A5** mở rộng ≥7 device | ⏸ Hoãn | cần nguồn fingerprint/TAC THẬT — không bịa (Global Constraints) |
| **A6** A.4 assert coherent trên MuMu | ✅ Code xong · ⏳ chờ hardware | ignored test; chỉ verify giá trị thật khi có MuMu |
| **B1/B2** SSAID qua restore (W3) | ⏸ Hoãn | cần MuMu thật dump `settings_ssaid.xml` (format/owner) trước khi code |
| **C1** `ProvisionHealth` tự soi tell (W6) | ✅ Xong | `is_fixable_tell` loại inherent + dedup Magisk; FE toast gate `attempted && !locked` |
| **W7/W8** root-hide / sensor / ARM | ⏸ Deferred | quyết định hạ tầng, không code trong plan này |

---

# PHASE A — Fingerprint coherence (W1 · W2 · W4 · W5)

*Cohesive, thuần trong `fingerprint.rs` + `adb.rs::build_lock_script` + wiring. Ship được độc lập. Đây là "Tầng 1" hiệu-quả-cao/công-sức-thấp.*

### Task A1: Thêm field coherent vào `HardwareProfile` (BE + FE parity)

**Files:**
- Modify: `src-tauri/src/model.rs` (struct `HardwareProfile`, ~line 27-45)
- Modify: `src/types/instance.ts` (interface `HardwareProfile`)
- Modify: `src/lib/mockBackend.ts` (`mockFingerprint`)

**Interfaces:**
- Produces: `HardwareProfile { …, soc_hardware: String, board_platform: String, gpu_egl: String, security_patch: String, build_characteristics: String }` (serde camelCase → `socHardware`, `boardPlatform`, `gpuEgl`, `securityPatch`, `buildCharacteristics`).

- [ ] **Step 1: Thêm 5 field (serde default) vào `HardwareProfile`**

Trong `src-tauri/src/model.rs`, sau field `build_fingerprint`:

```rust
    /// ro.hardware (SoC family, vd "qcom", "samsungexynos9810"). Rỗng = không set.
    #[serde(default)]
    pub soc_hardware: String,
    /// ro.board.platform (vd "sm6125", "exynos9810"). Rỗng = không set.
    #[serde(default)]
    pub board_platform: String,
    /// ro.hardware.egl ("mali" cho Exynos, "adreno" cho Snapdragon). Rỗng = không set.
    #[serde(default)]
    pub gpu_egl: String,
    /// ro.build.version.security_patch ("YYYY-MM-DD") khớp build. Rỗng = không set.
    #[serde(default)]
    pub security_patch: String,
    /// ro.build.characteristics của model thật (KHÔNG "tablet"). Rỗng = xóa prop.
    #[serde(default)]
    pub build_characteristics: String,
```

- [ ] **Step 2: Cập nhật mọi literal `HardwareProfile { … }` trong test để biên dịch**

Tìm các nơi khởi tạo `HardwareProfile { … }` bằng: `grep -rn "HardwareProfile {" src-tauri/src`. Ở mỗi test literal (orchestrator.rs `hw()`, db.rs `sample()`), thêm 5 field với giá trị mẫu (không rỗng để test coherence sau này), ví dụ trong `orchestrator.rs::tests::hw()`:

```rust
            build_fingerprint: "HUAWEI/FRD-L19/HWFRD:8.0.0/HUAWEIFRD-L19/380C431:user/release-keys"
                .into(),
            soc_hardware: "kirin950".into(),
            board_platform: "hi3650".into(),
            gpu_egl: "mali".into(),
            security_patch: "2018-01-01".into(),
            build_characteristics: "".into(),
```

- [ ] **Step 3: Thêm field vào FE `HardwareProfile` (optional để không vỡ dữ liệu cũ)**

Trong `src/types/instance.ts`, interface `HardwareProfile`, thêm:

```ts
  socHardware?: string;
  boardPlatform?: string;
  gpuEgl?: string;
  securityPatch?: string;
  buildCharacteristics?: string;
```

- [ ] **Step 4: `mockFingerprint` trả field mới**

Trong `src/lib/mockBackend.ts`, mỗi entry `devices[]` thêm (khớp bảng Rust ở Task A5, tạm để giá trị đại diện):

```ts
      socHardware: 'exynos8890',
      boardPlatform: '',
      gpuEgl: 'mali',
      securityPatch: '2018-11-01',
      buildCharacteristics: '',
```

Và trong object `return { ...d, imei, androidId, mac: '02:11:22:33:44:55' }` không cần đổi (spread đã gồm field mới).

- [ ] **Step 5: Biên dịch + test cổng**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -3` — Expected: `test result: ok.` (đã 62 pass, giữ nguyên)
Run: `npm run typecheck && npm run test 2>&1 | tail -4` — Expected: typecheck sạch, 13 test pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/model.rs src/types/instance.ts src/lib/mockBackend.ts src-tauri/src/orchestrator.rs src-tauri/src/db.rs
git commit -m "feat(anti-detect): them field coherent (soc/board/egl/patch/characteristics) vao HardwareProfile"
```

---

### Task A2: Tách `build_lock_script` thuần + set prop coherent (W1 + W2)

**Files:**
- Modify: `src-tauri/src/adb.rs` (`lock_device_identity` ~line 714-812; thêm `build_lock_script` cạnh `tar_archive_looks_valid`)
- Test: `src-tauri/src/adb.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `fn build_lock_script(rp: &str, hw: &HardwareProfile) -> String` — sinh nội dung script resetprop; `lock_device_identity` gọi nó thay cho khối inline.

- [ ] **Step 1: Viết test thất bại cho `build_lock_script`**

Thêm vào `mod tests` cuối `adb.rs`:

```rust
    fn hw_lock() -> HardwareProfile {
        HardwareProfile {
            model: "Redmi Note 8".into(),
            brand: "Redmi".into(),
            manufacturer: "Xiaomi".into(),
            imei: "861000000000000".into(),
            android_id: "a1b2c3d4e5f60718".into(),
            mac: "02:00:00:11:22:33".into(),
            res_width: 1080,
            res_height: 2340,
            dpi: 440,
            device: "ginkgo".into(),
            build_fingerprint: "Redmi/ginkgo/ginkgo:11/RP1A.200720.011/V12.5.1.0.RCOMIXM:user/release-keys".into(),
            soc_hardware: "qcom".into(),
            board_platform: "trinket".into(),
            gpu_egl: "adreno".into(),
            security_patch: "2021-05-01".into(),
            build_characteristics: "".into(),
        }
    }

    #[test]
    fn build_lock_script_giu_gia_tri_co_khoang_trang_va_coherent() {
        let s = build_lock_script("magisk resetprop", &hw_lock());
        // Value có khoảng trắng phải được bọc nháy đơn nguyên vẹn.
        assert!(s.contains("magisk resetprop ro.product.model 'Redmi Note 8'"), "{s}");
        // Prop coherent set từ profile.
        assert!(s.contains("magisk resetprop ro.hardware 'qcom'"));
        assert!(s.contains("magisk resetprop ro.board.platform 'trinket'"));
        assert!(s.contains("magisk resetprop ro.hardware.egl 'adreno'"));
        assert!(s.contains("magisk resetprop ro.build.version.security_patch '2021-05-01'"));
        // characteristics rỗng -> XÓA (chống tell 'tablet'), không set giá trị bịa.
        assert!(s.contains("magisk resetprop --delete ro.build.characteristics"));
        assert!(!s.contains("tablet"), "khong duoc de lai tell tablet");
        // Prop rỗng bị bỏ qua (không sinh dòng set rỗng).
        let hw_empty = HardwareProfile { soc_hardware: "".into(), ..hw_lock() };
        let s2 = build_lock_script("resetprop", &hw_empty);
        assert!(!s2.contains("resetprop ro.hardware '"), "rong phai bo qua, khong set");
    }
```

- [ ] **Step 2: Chạy test → xác nhận FAIL**

Run: `cd src-tauri && cargo test --lib adb::tests::build_lock_script -- --nocapture`
Expected: FAIL — `cannot find function build_lock_script`.

- [ ] **Step 3: Thêm hàm thuần `build_lock_script` (đặt trên `impl AdbWorker for RealAdbWorker`)**

```rust
/// Sinh nội dung script resetprop (thuần — test được không cần MuMu). Value có khoảng
/// trắng được bọc nháy đơn (sh đọc từ file giữ nguyên). Prop rỗng bị BỎ QUA (nguyên tắc
/// #07: thà không set còn hơn set giá trị mâu thuẫn). `ro.build.characteristics` rỗng →
/// XÓA (gỡ tell 'tablet' của MuMu) thay vì set bịa.
fn build_lock_script(rp: &str, hw: &HardwareProfile) -> String {
    let esc = |v: &str| v.replace('\'', "'\\''");
    let mut s = String::from("#!/system/bin/sh\n");

    // 1) Xóa prop QEMU/MuMu.
    for p in ["ro.kernel.qemu", "ro.boot.qemu", "ro.mumu.version"] {
        s.push_str(&format!("{rp} --delete {p}\n"));
    }

    // 2) Định danh lõi (giữ nguyên hành vi hiện tại).
    let core: [(&str, &str); 7] = [
        ("ro.product.model", &hw.model),
        ("ro.product.brand", &hw.brand),
        ("ro.product.manufacturer", &hw.manufacturer),
        ("ro.product.device", &hw.device),
        ("ro.product.name", &hw.device),
        ("ro.build.fingerprint", &hw.build_fingerprint),
        ("ro.product.board", &hw.device),
    ];
    for (k, v) in core {
        if !v.is_empty() {
            s.push_str(&format!("{rp} {k} '{}'\n", esc(v)));
        }
    }

    // 3) Prop coherent MỚI (bỏ qua nếu rỗng). characteristics rỗng → xóa tell tablet.
    let coherent: [(&str, &str); 5] = [
        ("ro.hardware", &hw.soc_hardware),
        ("ro.board.platform", &hw.board_platform),
        ("ro.hardware.egl", &hw.gpu_egl),
        ("ro.build.version.security_patch", &hw.security_patch),
        ("ro.build.characteristics", &hw.build_characteristics),
    ];
    for (k, v) in coherent {
        if v.is_empty() {
            if k == "ro.build.characteristics" {
                s.push_str(&format!("{rp} --delete {k}\n"));
            }
            continue;
        }
        s.push_str(&format!("{rp} {k} '{}'\n", esc(v)));
    }

    // 4) Build type user/release-keys + ẩn adb.
    let build_props: [(&str, &str); 4] = [
        ("ro.build.tags", "release-keys"),
        ("ro.build.type", "user"),
        ("ro.secure", "1"),
        ("ro.debuggable", "0"),
    ];
    for (k, v) in build_props {
        s.push_str(&format!("{rp} {k} '{v}'\n"));
    }
    s.push_str(&format!("{rp} sys.usb.state 'mtp'\n"));

    // 5) Ẩn file/device node QEMU (bind /dev/null nếu tồn tại).
    for f in [
        "/dev/qemu_pipe", "/dev/socket/qemud", "/dev/socket/genyd",
        "/system/lib/vboxguest.ko", "/system/bin/nemuVM-tools", "/system/xbin/nemuVM-tools",
    ] {
        s.push_str(&format!("if [ -e {f} ]; then mount -o bind /dev/null {f}; fi\n"));
    }
    s
}
```

- [ ] **Step 4: Dùng `build_lock_script` trong `lock_device_identity`**

Trong `lock_device_identity`, THAY toàn bộ đoạn dựng `script` inline (từ `let mut script = String::from("#!/system/bin/sh\n");` tới hết khối `hide_files`) bằng:

```rust
        let script = build_lock_script(&rp, hw);
```

Giữ nguyên phần ghi file host, push, chạy `sh <remote>`, verify model+fingerprint, cleanup phía dưới.

- [ ] **Step 5: Chạy test → PASS**

Run: `cd src-tauri && cargo test --lib adb::tests::build_lock_script -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Gỡ khối `sed`/remount chết trong `harden()` (W1)**

Trong `adb.rs::harden`, XÓA đoạn (chết trong disposable — đã chuyển sang resetprop):

```rust
        // Sửa ro.build.characteristics qua build.prop (cần root + remount; ăn sau reboot).
        let _ = self
            .adb(idx, "shell su -c \"mount -o rw,remount /system; sed -i 's/ro.build.characteristics=tablet/ro.build.characteristics=default/' /system/build.prop\"")
            .await;
```

- [ ] **Step 7: Cổng + commit**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | tail -2`
Expected: test ok, clippy `Finished` không warning.

```bash
git add src-tauri/src/adb.rs
git commit -m "feat(anti-detect): khoa ro.build.characteristics+SoC/GPU/patch runtime qua resetprop; go sed chet (W1/W2)"
```

---

### Task A3: IMEI khớp TAC nhà sản xuất (W4 / #47)

**Files:**
- Modify: `src-tauri/src/fingerprint.rs` (`DeviceProfile` struct, `gen_imei`, `generate`)
- Test: `src-tauri/src/fingerprint.rs` (`mod tests`)

**Interfaces:**
- Produces: `fn gen_imei(tac: &str) -> AppResult<String>` — nếu `tac` đủ 8 chữ số thì ghép `tac + 6 serial random + Luhn`; ngược lại random 14 số (hành vi cũ, an toàn khi chưa có TAC thật).

- [ ] **Step 1: Viết test thất bại**

Thêm vào `fingerprint.rs::tests`:

```rust
    #[test]
    fn imei_dung_tac_khi_co() {
        let imei = gen_imei("35847209").unwrap();
        assert_eq!(imei.len(), 15);
        assert_eq!(&imei[..8], "35847209", "8 so dau phai la TAC");
        assert!(luhn_valid(&imei));
    }

    #[test]
    fn imei_random_khi_tac_rong() {
        let imei = gen_imei("").unwrap();
        assert_eq!(imei.len(), 15);
        assert!(luhn_valid(&imei));
    }
```

- [ ] **Step 2: Chạy → FAIL**

Run: `cd src-tauri && cargo test --lib fingerprint::tests::imei_dung_tac -- --nocapture`
Expected: FAIL — `gen_imei` hiện không nhận tham số.

- [ ] **Step 3: Đổi `gen_imei` nhận `tac` + thêm `tac` vào `DeviceProfile`**

Sửa `gen_imei`:

```rust
fn gen_imei(tac: &str) -> AppResult<String> {
    let tac_digits: Vec<u8> = tac.chars().filter_map(|c| c.to_digit(10)).map(|d| d as u8).collect();
    let mut digits: Vec<u8> = Vec::with_capacity(14);
    if tac_digits.len() == 8 {
        digits.extend_from_slice(&tac_digits);
        digits.extend(rand_bytes(6)?.iter().map(|b| b % 10));
    } else {
        digits.extend(rand_bytes(14)?.iter().map(|b| b % 10));
    }
    let check = luhn_check_digit(&digits);
    let mut s: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    s.push((b'0' + check) as char);
    Ok(s)
}
```

Thêm field vào `struct DeviceProfile` (dưới `dpi`):

```rust
    /// 8 số TAC THẬT của model (nguồn: getprop máy thật/DB TAC). Rỗng = IMEI random.
    tac: &'static str,
    soc_hardware: &'static str,
    board_platform: &'static str,
    gpu_egl: &'static str,
    security_patch: &'static str,
    build_characteristics: &'static str,
```

- [ ] **Step 4: Cập nhật `generate()` truyền `tac` + map field mới**

Trong `generate()`:

```rust
    Ok(HardwareProfile {
        model: d.model.to_string(),
        brand: d.brand.to_string(),
        manufacturer: d.manufacturer.to_string(),
        imei: gen_imei(d.tac)?,
        android_id: gen_android_id()?,
        mac: gen_mac()?,
        res_width: d.w,
        res_height: d.h,
        dpi: d.dpi,
        device: d.device.to_string(),
        build_fingerprint: d.fingerprint.to_string(),
        soc_hardware: d.soc_hardware.to_string(),
        board_platform: d.board_platform.to_string(),
        gpu_egl: d.gpu_egl.to_string(),
        security_patch: d.security_patch.to_string(),
        build_characteristics: d.build_characteristics.to_string(),
    })
```

- [ ] **Step 5: Điền field mới cho 5 entry `DEVICES` (giá trị THẬT đã biết; chưa chắc để RỖNG)**

> **Quy tắc dữ liệu (Global Constraints):** chỉ điền giá trị xác minh được. `gpu_egl`/`security_patch`/`build_characteristics` điền theo họ SoC + mốc build (đủ tin). `soc_hardware`/`board_platform`/`tac` — điền nếu chắc, **để rỗng** nếu chưa (code bỏ qua an toàn). Cổng A.4 sẽ xác nhận trên máy thật ở Task A6.

Cập nhật mỗi entry, ví dụ Redmi Note 8 (Snapdragon 665 → GPU Adreno; đủ tin):

```rust
    DeviceProfile {
        model: "Redmi Note 8",
        brand: "Redmi",
        manufacturer: "Xiaomi",
        device: "ginkgo",
        fingerprint: "Redmi/ginkgo/ginkgo:11/RP1A.200720.011/V12.5.1.0.RCOMIXM:user/release-keys",
        w: 1080, h: 2340, dpi: 440,
        tac: "",                       // TODO xác minh TAC thật của Redmi Note 8; rỗng = random
        soc_hardware: "qcom",          // Snapdragon → "qcom" (đã xác minh qua scan MuMu)
        board_platform: "",            // TODO SD665 board (trinket/sm6125) — xác minh
        gpu_egl: "adreno",             // Adreno 610
        security_patch: "2021-05-01",  // suy từ RP1A.200720 + MIUI 12.5.1
        build_characteristics: "",     // xóa tell tablet (Redmi phone: characteristics rỗng)
    },
```

Các entry Samsung (Exynos → `gpu_egl: "mali"`), điền `security_patch` theo build id, để `soc_hardware`/`board_platform`/`tac` rỗng nếu chưa xác minh chuỗi chính xác.

- [ ] **Step 6: Sửa test cũ gọi `gen_imei` (nếu có) + chạy toàn bộ fingerprint test**

Run: `cd src-tauri && cargo test --lib fingerprint 2>&1 | tail -6`
Expected: tất cả test fingerprint PASS (kể cả `imei_15_so_va_hop_le_luhn`, `moi_lan_sinh_khac_nhau`).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/fingerprint.rs
git commit -m "feat(anti-detect): IMEI ghep TAC that + field coherent trong DeviceProfile (W4)"
```

---

### Task A4: Khử lệch modulo khi chọn device (W5 / #48)

**Files:**
- Modify: `src-tauri/src/fingerprint.rs` (`generate`, thêm `pick_device_index`)
- Test: `src-tauri/src/fingerprint.rs` (`mod tests`)

**Interfaces:**
- Produces: `fn pick_device_index(n: usize) -> AppResult<usize>` — chọn 0..n **không lệch modulo** bằng rejection sampling.

- [ ] **Step 1: Viết test thất bại (phân bố không lệch)**

```rust
    #[test]
    fn chon_device_khong_lech_modulo() {
        // n=5: với rejection sampling, mọi index 0..5 đều khả dĩ, không index nào bị chặn.
        let mut seen = [false; 5];
        for _ in 0..500 {
            let i = pick_device_index(5).unwrap();
            assert!(i < 5);
            seen[i] = true;
        }
        assert!(seen.iter().all(|&b| b), "moi device deu phai duoc chon it nhat 1 lan");
    }
```

- [ ] **Step 2: Chạy → FAIL**

Run: `cd src-tauri && cargo test --lib fingerprint::tests::chon_device -- --nocapture`
Expected: FAIL — `pick_device_index` chưa tồn tại.

- [ ] **Step 3: Thêm `pick_device_index` + dùng trong `generate`**

```rust
/// Chọn index 0..n không lệch modulo (rejection sampling trên byte 0..256).
fn pick_device_index(n: usize) -> AppResult<usize> {
    assert!(n > 0 && n <= 256);
    let limit = (256 / n) * n; // bội lớn nhất của n ≤ 256
    loop {
        let b = rand_bytes(1)?[0] as usize;
        if b < limit {
            return Ok(b % n);
        }
    }
}
```

Trong `generate()`, thay `let pick = rand_bytes(1)?[0] as usize % DEVICES.len();` bằng:

```rust
    let pick = pick_device_index(DEVICES.len())?;
```

- [ ] **Step 4: Chạy → PASS**

Run: `cd src-tauri && cargo test --lib fingerprint 2>&1 | tail -4`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/fingerprint.rs
git commit -m "feat(anti-detect): rejection sampling chon device khu lech modulo (W5)"
```

---

### Task A5: Mở rộng bộ device (giảm trùng) — data task có kiểm

**Files:**
- Modify: `src-tauri/src/fingerprint.rs` (`DEVICES`)
- Modify: `src/lib/mockBackend.ts` (đồng bộ vài entry cho demo — không bắt buộc đủ)

**Interfaces:**
- Consumes: `DeviceProfile` (Task A3).

- [ ] **Step 1: Thu thập ≥7 bộ fingerprint THẬT bổ sung (đời cũ, coherent)**

Nguồn xác minh (Global Constraints): `getprop` máy thật hoặc `u0pattern/Anti-EmuDetector`/`PlayIntegrityFork`. Mỗi bộ cần khớp: `model/brand/manufacturer/device/fingerprint/tac/soc_hardware/board_platform/gpu_egl/security_patch` + `w/h/dpi` (rộng≤1080, dpi≤480 — ràng buộc MuMu-an-toàn ghi trong fingerprint.rs). Ghi nguồn từng bộ vào comment.

- [ ] **Step 2: Thêm entry vào `DEVICES` (điền field chắc, rỗng field chưa chắc)**

Ví dụ 1 entry mẫu (đủ tin ở mức họ SoC/GPU/patch):

```rust
    // Samsung Galaxy S10e (SM-G970F, beyond0lte, Exynos 9820, Mali-G76, Android 12).
    DeviceProfile {
        model: "SM-G970F", brand: "samsung", manufacturer: "samsung", device: "beyond0lte",
        fingerprint: "samsung/beyond0ltexx/beyond0lte:12/SP1A.210812.016/G970FXXSGHVK2:user/release-keys",
        w: 1080, h: 2280, dpi: 438,
        tac: "", soc_hardware: "", board_platform: "",
        gpu_egl: "mali", security_patch: "2022-11-01", build_characteristics: "",
    },
```

- [ ] **Step 3: Chạy test (đảm bảo mọi entry coherent + build)**

Run: `cd src-tauri && cargo test --lib fingerprint 2>&1 | tail -4`
Expected: PASS. `chon_device_khong_lech_modulo` cập nhật `[false; 5]` → `[false; DEVICES.len()]` nếu cần (hoặc đổi test dùng `DEVICES.len()`).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/fingerprint.rs src/lib/mockBackend.ts
git commit -m "feat(anti-detect): mo rong bo device coherent giam trung (W5)"
```

---

### Task A6: Cổng coherence trên MuMu thật — mở rộng A.4

**Files:**
- Modify: `src-tauri/src/e2e_real.rs` (test `a4_*` fingerprint/runtime tells)

**Interfaces:**
- Consumes: `lock_device_identity` (đã set prop coherent), `generate()`.

- [ ] **Step 1: Thêm assertion getprop cho prop mới vào A.4**

Trong test A.4 (sau khi provision + lock), thêm đọc-lại và cảnh báo/assert (theo phong cách "known-gap warn" nếu chưa chắc, "assert" nếu chắc):

```rust
        // W1: characteristics KHÔNG được là tablet.
        let characteristics = worker.prop_public(idx, "ro.build.characteristics").await; // hoặc adb getprop
        assert!(!characteristics.contains("tablet"), "ro.build.characteristics van la tablet: {characteristics}");

        // W2: GPU egl khớp họ (mali cho Samsung, adreno cho Snapdragon) nếu profile có set.
        if !hw.gpu_egl.is_empty() {
            let egl = /* getprop ro.hardware.egl */;
            assert_eq!(egl.trim(), hw.gpu_egl, "ro.hardware.egl lech profile");
        }
        // security_patch nếu có set.
        if !hw.security_patch.is_empty() {
            let patch = /* getprop ro.build.version.security_patch */;
            assert_eq!(patch.trim(), hw.security_patch, "security_patch lech profile");
        }
```

> Dùng đúng helper getprop có sẵn trong `e2e_real.rs` (khớp cách A.4 hiện đọc model/fingerprint). Với prop để rỗng trong profile thì bỏ qua assert (đúng nguyên tắc omit).

- [ ] **Step 2: Chạy trên MuMu thật (thủ công, ignored)**

Run: `cd src-tauri && cargo test --lib e2e_real::a4 -- --ignored --nocapture`
Expected: PASS trên máy có MuMu; ghi lại giá trị getprop thực để đối chiếu/điền tiếp các field còn rỗng (W2/W4/W5).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/e2e_real.rs
git commit -m "test(anti-detect): A.4 assert characteristics/egl/security_patch coherent tren MuMu that"
```

---

# PHASE B — Bảo toàn android_id/SSAID qua restore (W3)

*Cần spike trên MuMu thật: định dạng `settings_ssaid.xml` và đường ghi phụ thuộc Android version của base image. Ship sau Phase A.*

### Task B1: Spike — xác định vị trí + format SSAID của TikTok trên base image

**Files:** none (điều tra), ghi kết quả vào `docs/ANTI_DETECTION_UPGRADE.md`.

- [ ] **Step 1: Provision 1 VM, cài + mở TikTok, dump SSAID**

Run các lệnh qua `MuMuManager.exe adb -v <idx> -c`:
```
shell su -c 'cat /data/system/users/0/settings_ssaid.xml'
shell su -c 'stat -c %U:%G /data/system/users/0/settings_ssaid.xml'
```
Ghi: TikTok (`com.zhiliaoapp.musically`) có entry SSAID không, format `<setting id=.. name=<uid> value=<hex16> package=<pkg> .../>`, owner/label.

- [ ] **Step 2: Ghi phát hiện + quyết định hướng vào docs**

Chọn 1 trong 2:
- **(a) Backup+restore entry SSAID** cùng `/data/data` (giữ đúng giá trị TikTok từng thấy), hoặc
- **(b) Ghi SSAID xác định từ `hw.android_id`** vào `settings_ssaid.xml` trước start app (đơn giản, nhất quán per-account).

Ghi rõ format + owner/restorecon vào `docs/ANTI_DETECTION_UPGRADE.md`.

- [ ] **Step 3: Commit tài liệu spike**

```bash
git add docs/ANTI_DETECTION_UPGRADE.md
git commit -m "docs(anti-detect): spike SSAID location/format cho W3"
```

### Task B2: Triển khai theo hướng đã chốt + e2e round-trip

**Files:**
- Modify: `src-tauri/src/adb.rs` (thêm bước SSAID vào `backup`/`restore` HOẶC hàm `apply_ssaid`)
- Test: `src-tauri/src/e2e_real.rs` (biến thể A.13: android_id ổn định qua restore)

- [ ] **Step 1: Viết e2e thất bại (android_id ổn định qua stop→run VM mới)**

Trong `e2e_real.rs`, test ignored: create→run→đọc `settings get secure android_id` (dưới uid TikTok nếu đọc được, hoặc so với `hw.android_id`)→stop→run lại→android_id phải KHỚP.

- [ ] **Step 2: Chạy → FAIL** (`cargo test --lib e2e_real::a13_ssaid -- --ignored --nocapture`).

- [ ] **Step 3: Triển khai hướng (a) hoặc (b) từ B1** — thêm đóng gói/khôi phục hoặc ghi `settings_ssaid.xml` với `chown system:system` + `restorecon`.

- [ ] **Step 4: Chạy → PASS trên MuMu thật.**

- [ ] **Step 5: Commit** (`feat(anti-detect): bao toan android_id/SSAID qua restore (W3)`).

---

# PHASE C — Tự soi dấu vết cuối provision (W6, mở rộng #26)

### Task C1: `ProvisionHealth` gộp lock + fixable tells

**Files:**
- Modify: `src-tauri/src/model.rs` (struct `ProvisionHealth`, đổi `RunProfileResult`)
- Modify: `src-tauri/src/orchestrator.rs` (scan cuối provision → lọc tell sửa-được còn detected)
- Modify: `src-tauri/src/profile_ops.rs`, `commands.rs`
- Modify: `src/types/instance.ts`, `src/store/useProfileStore.ts`, `src/features/profiles/ProfilesView.tsx`

**Interfaces:**
- Produces: `struct ProvisionHealth { fingerprint_lock: FingerprintLockStatus, fixable_tells: Vec<String> }`; `RunProfileResult { vm_index, health: ProvisionHealth }`.

- [ ] **Step 1: Định nghĩa `ProvisionHealth`, thay field trong `RunProfileResult`** (model.rs, serde camelCase). Fixable tells = các `EmulatorTell.detected==true` thuộc nhóm SỬA ĐƯỢC (loại native-bridge/hypervisor/sensor — nền tảng).

- [ ] **Step 2: Cuối `provision`, gọi `scan_emulator_tells`, lọc fixable detected → gắn vào state theo index** (giống cách #26 lưu `fingerprint_lock`). Thêm getter/cleanup trong `forget`.

- [ ] **Step 3: `run` đọc `ProvisionHealth`, `run_profile` trả về; FE `ProfilesView.doRun` cảnh báo nếu `health.fixableTells` không rỗng** (mở rộng toast `!locked` hiện có).

- [ ] **Step 4: Cổng** `cargo test --lib && cargo clippy` + `npm run typecheck && npm run test`.

- [ ] **Step 5: Commit** (`feat(anti-detect): ProvisionHealth tu soi tell sua-duoc cuoi provision (W6)`).

---

# DEFERRED — Quyết định hạ tầng (W7 · W8), KHÔNG code trong plan này

Các mục sau **không sửa được ở tầng app/property standalone** — ghi lại để quyết định riêng, không đưa vào code plan:

- **W7 Full root-hide (Shamiko/Zygisk/DenyList):** cần base image MuMu có **full Magisk + Zygisk**, cài Shamiko, thêm TikTok vào DenyList, **tắt Enforce DenyList**, reboot. Đo bằng `cargo test --lib e2e_real::a15_magisk_shamiko_root_hide_diagnostics -- --ignored --nocapture` (đã có). MPM chỉ dùng resetprop standalone → không đủ. (Tùy chọn nhỏ: dọn `/data/local/tmp/magisk` sau lần re-assert cuối — cân nhắc thứ tự gọi để không hỏng re-assert sau install/restore.)
- **W8 Sensor entropy + native-bridge/hypervisor:** sensor cần **Sensor HAL/bridge trong base image** (accel/gyro/từ kế tương quan vật lý theo `sensor_seed`); native-bridge+hypervisor **cố hữu x86** — chỉ hết khi **pivot ARM image/redroid-ARM**. Đo bằng A.16 + scan. Đây là quyết định hạ tầng (ROI), không phải thêm module.

---

## Self-Review (đã chạy)

- **Spec coverage:** W1→A2/A6 · W2→A1/A2/A6 · W4→A3 · W5→A4/A5 · #49 (uniqueness) → *chưa có task riêng* (rủi ro trùng RNG cực nhỏ; nếu cần, thêm task A7: `create()` sinh lại khi imei/android_id trùng profile đã lưu) · W3→B1/B2 · W6→C1 · W7/W8→Deferred. **Gap ghi nhận:** #49 cố ý để tùy chọn; #40 (cache magisk theo nội dung) không thuộc anti-detect core — nằm ở plan bug cũ.
- **Placeholder scan:** các giá trị `tac`/`board_platform` để **rỗng** là quyết định thiết kế có chủ đích (code bỏ qua an toàn) + có cổng A.4/A5 điền giá trị thật — KHÔNG phải placeholder logic.
- **Type consistency:** `build_lock_script(rp, hw)` dùng nhất quán A2; `gen_imei(tac)` A3; `pick_device_index(n)` A4; `HardwareProfile` 5 field mới nhất quán A1↔A3↔A2; `RunProfileResult.health` C1 thay `fingerprint_lock` — cập nhật mọi nơi đọc (`vm_index` giữ nguyên).

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-08-anti-detection-completion.md`. Hai lựa chọn thực thi:

1. **Subagent-Driven (khuyến nghị)** — dispatch subagent mới cho từng task, review giữa các task, lặp nhanh.
2. **Inline Execution** — chạy các task trong phiên này (executing-plans), theo lô có checkpoint để review.

Chọn cách nào?
