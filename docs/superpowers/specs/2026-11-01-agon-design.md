# Desain implementasi Agon v1

Status: usulan desain untuk persetujuan. Sumber kebutuhan: `PRD.md` (1 November 2026).

## Tujuan dan batas

Membangun aplikasi timing F1 dengan backend Rust, frontend React/TypeScript, replay historis sejak 2023, telemetry, chart perbandingan, dan prediksi yang menjelaskan ketidakpastiannya. Target pengguna dan alasan produk mengikuti PRD: penggemar dapat memahami strategi tanpa menghitung sendiri dari timing mentah.

Seluruh fitur Must dan Should dalam PRD masuk cakupan v1. Fitur Could P7/P8/P11 dibuat hanya jika diperlukan sebagai input simulasi atau penyajian insight berbasis aturan; classifier SC/overtake dan LLM narrator bukan syarat penerimaan v1. Video, audio radio, taruhan, akun, monetisasi, aplikasi native, dan branding resmi tidak dibangun.

Tidak ada data lomba atau skor akurasi rekaan yang dipresentasikan sebagai data nyata. Fixture sintetis hanya untuk pengujian/demo dan diberi label jelas. Tanpa kredensial live, replay historis tetap dapat digunakan; status live menjelaskan keterbatasan akses. Ketika model ONNX tidak tersedia/gagal, prediksi menggunakan fallback PRD dengan label **basic estimate**. Fallback adalah jalur ketahanan operasional, bukan pengganti pekerjaan pelatihan P1/P3 dan backtest yang diwajibkan PRD. Keberhasilan integrasi bukan bukti tercapainya target akurasi, kapasitas, atau SLA produksi; kekurangan bagian wajib tetap dilaporkan sebagai implementasi parsial.

## Pendekatan

1. **React/TypeScript + Rust (dipilih berdasarkan persetujuan rekomendasi).** Mendukung iterasi UI dan pengujian browser dengan cepat; tipe kontrak diekspor dari Rust melalui `ts-rs` agar tidak ditulis ulang.
2. **Leptos/WASM + Rust.** Berbagi bahasa seluruh stack, tetapi menambah pekerjaan tooling frontend dan integrasi chart. Tidak dipakai untuk implementasi ini.

Penyajian awal menggunakan satu proses server Rust dengan pemisahan crate sesuai PRD. Pemisahan deployment per layanan hanya dilakukan bila hasil pengukuran memerlukannya. Ini tetap memakai kepemilikan domain yang sama tanpa menambah komunikasi antarlayanan pada jalur lokal.

## Keputusan implementasi

- Pengguna anonim; layout dan preferensi disimpan lokal. UI berbahasa Inggris mengikuti materi PRD; waktu memakai zona waktu browser. Tidak ada akun atau sinkronisasi lintas perangkat.
- OpenF1 menjadi sumber timing/telemetry eksternal v1. Akses live memakai transport streaming resmi yang terverifikasi saat integrasi. Tidak mengimplementasikan feed SignalR tidak resmi. Metadata statis sirkuit dikelola dalam dataset versi-terkontrol dengan sumber, lisensi, tahun/konfigurasi, dan pemetaan koordinat yang dapat diperiksa; sumber metadata terverifikasi boleh melengkapi OpenF1 tanpa menambah feed timing kedua.
- Frontend React/TypeScript dengan Vite; ECharts untuk chart interaktif dan PNG, SVG/canvas untuk peta. Tidak memerlukan raster atau logo resmi.
- Backend Cargo workspace: `pitwall-core`, `pitwall-ingest`, `pitwall-state`, `pitwall-predict`, `pitwall-api`, dan `pitwall-store`. Frontend berada di `pitwall-web` sebagai paket TypeScript, sesuai opsi fallback PRD.
- PostgreSQL menyimpan sesi, data terstruktur yang dibutuhkan untuk replay, serta log prediksi dan aktual. TimescaleDB menjadi ekstensi deployment untuk histori time-series. Redis menyimpan snapshot dan mendukung fan-out antarnode. Parquet untuk dataset pelatihan/ekspor offline, bukan endpoint redistribusi raw feed publik.
- Runtime model ONNX berada di Rust. Python hanya untuk akuisisi dataset, pelatihan, ekspor model, serta backtest offline. Tidak ada layanan Python pada jalur live.
- Docker Compose menyediakan lingkungan lokal yang dapat direproduksi; server juga dapat dijalankan langsung dengan Rust. CI menjalankan format, clippy, test, typecheck/build, dan e2e replay.

Keputusan di atas adalah pilihan implementasi, bukan tambahan kebutuhan produk. Versi dependency dan detail internal yang tidak mengubah kontrak ditentukan saat implementasi dari dukungan tooling aktual.

## Alur data dan invariant

`OpenF1 / berkas replay -> ingest -> state sesi -> delta timing -> API WebSocket -> browser`

`event lap/pit/status/cuaca -> predict -> log ber-versi -> delta prediksi -> browser`

1. Ingest menormalisasi event menjadi domain bersama; `DataSource` trait mengisolasi sumber OpenF1 dan berkas replay sebagaimana diwajibkan PRD. Respons eksternal, event, dan query klien divalidasi pada batas masuk.
2. State dipisahkan per sesi; urutan waktu, identitas event, lap, stint, pit, dan status dijaga. Event duplikat tidak menggandakan lap/stint. Telemetry menggunakan ring buffer terbatas.
3. Timing dipublikasikan segera setelah state diterapkan. Inferensi tidak memblokir timing. Prediksi membawa waktu input dan versi state sehingga hasil pekerjaan lama tidak mengganti hasil state baru.
4. Satu koneksi WebSocket per browser menerima snapshot penuh lalu delta MessagePack dengan sequence. Subscription telemetry hanya aktif ketika panel driver terbuka. Klien lambat tidak menciptakan antrean tanpa batas; sequence terlewat memicu snapshot ulang.
5. Reconnect mengirim snapshot terbaru. Putus sumber menahan state terakhir dan menampilkan **data delayed**. Kunci sumber hanya di server, tidak di bundle, URL bagikan, log, atau local storage.
6. Replay berjalan dari jam sesi yang dikontrol server, bukan jam browser. Seek membatalkan pekerjaan lama, membangun ulang state sampai timestamp tujuan, lalu mengirim snapshot baru. Tidak ada lap mendatang yang menjadi input prediksi. Replay per pengguna/koneksi terisolasi agar kontrol satu pengguna tidak mengubah sesi pengguna lain.
7. Kecepatan replay 1x–20x mengubah jam pemutaran; hasil prediksi pada timestamp yang sama tetap dapat direproduksi dengan model dan seed simulasi yang sama. Cache historis dipakai ulang dengan pembatasan akses OpenF1.
8. Prediksi selalu membawa band/probabilitas, confidence, tiga faktor utama, label model/fallback, dan status. Red flag menghentikan prediksi; SC/VSC/cuaca berubah menurunkan confidence. Tidak menampilkan nilai yang tidak tersedia sebagai nol.
9. Confidence fallback menunjukkan ketidakpastian estimasi, bukan probabilitas terkalibrasi yang belum dibuktikan. Skor akurasi hanya dihitung dari forecast yang dicatat sebelum aktual terjadi.

## Antarmuka dan alur pengguna

### Memilih sesi dan playback

Pengguna memilih tahun (mulai 2023), event, dan sesi dari katalog OpenF1. Mode Practice, Qualifying, Race, dan Sprint berasal dari metadata sesi. Status loading, sumber tidak tersedia, sesi tanpa data, dan retry ditampilkan secara eksplisit. Replay menyediakan play/pause, 1x–20x, posisi waktu/lap, dan scrubber. Live memakai sesi aktif dan menampilkan umur data.

Header menampilkan event/sesi, lap atau clock yang sesuai, status lintasan, serta cuaca (track/air/humidity/wind/rain) jika ada. Tidak mengarang total lap atau konfigurasi sirkuit yang tidak tersedia. Dataset sirkuit mencakup corner, sektor/mini-sektor, zona DRS, pit lane, flag zones, dan pemetaan distance/koordinat untuk konfigurasi sesi sejak 2023. Jumlah lap berasal dari konfigurasi sesi; pit loss dikalibrasi dari histori dan membawa band/asumsi. Metadata harus bersumber jelas. Unavailable merupakan error/partial-data state, bukan bukti bahwa fitur metadata terkait sudah memenuhi PRD. Kelengkapan dataset diverifikasi terhadap katalog sesi yang didukung sebelum klaim replay semua sesi.

### Timing tower, map, dan race control

- Desktop: tower di kiri, peta di kanan, race control dan chart yang dipin di area pendamping. Tablet dua kolom; ponsel satu panel Tower/Map/Charts dengan navigasi sentuh dan keyboard.
- Tower memuat seluruh kolom §4.1: posisi/perubahan, kode/nomor/warna driver, gap/interval, last/best lap, sektor dan mini-sektor, tyre/age/stint/new-used, pit/count/duration, pace band, pit window/probability, tyre health/cliff, dan projected finish beserta ketidakpastian.
- Pengguna dapat menyembunyikan dan mengurutkan kolom, lalu layout tersimpan lokal. Gap/interval dapat diganti. DRS range dan waktu tercepat memakai teks/ikon selain warna.
- Practice menekankan best lap/stint; qualifying menampilkan cut line dan perkiraan kebutuhan waktu dengan confidence; Race/Sprint menekankan gap, pit, dan strategi.
- Peta dibentuk dari location yang tersedia, memakai warna tim generik tanpa logo. Interpolasi antarsampel dibatasi saat sumber terlambat. Hover/focus menampilkan driver/gap; klik membuka panel. Tampilkan sektor, flag zones, DRS, pit lane, dan ghost rejoin memakai metadata konfigurasi sesi. Mode fastest-driver mini-sectors mewarnai segmen berdasarkan perbandingan lap terpilih; hasil turunan dari sampel jarang diberi label approximate. Ketidaktersediaan metadata terlihat sebagai kondisi data parsial.
- Race control menampilkan flags, penalties, investigations, track limits, dan SC; dapat difilter per driver. Isi pesan eksternal dirender sebagai teks.

### Telemetry driver

Klik satu driver membuka side panel desktop atau sheet penuh ponsel. Tutup/pindah driver menghentikan subscription sebelumnya. Panel memiliki fokus keyboard yang benar dan mengembalikan fokus saat ditutup.

1. **At a glance:** speed, gear, RPM, throttle, brake, DRS; stint, compound, age, deg, cliff, pit window; posisi/gap/trend dan insight aturan yang bersumber dari data.
2. **Lap traces:** speed/throttle/brake/gear/RPM terhadap distance dengan satu x-axis, delta-time, overlay lap sendiri sebelumnya/best atau driver lain. Distance diturunkan dari integrasi speed dan dinormalisasi bila sumber tidak menyediakan distance langsung; gap data tidak disambungkan seolah kontinu. Traces dilabeli approximate (~3.7 Hz), corner markers bersumber metadata, hover terkait mini-map.
3. **Corner breakdown:** braking distance, apex speed, exit to full throttle, dan delta terhadap referensi, diberi label estimasi. Jika resolusi/metadata tidak mencukupi, tampilkan unavailable dan alasannya.
4. **History:** lap pace dengan stint/pit markers, tabel tyre history/average pace/deg, speed trap/sector best, posisi per lap, dan race control driver.

Beginner menampilkan layer 1–2; Expert membuka layer 3–4. Semua chart memiliki penjelasan satu kalimat. Tidak menyimpulkan kerusakan sebagai fakta dari anomaly statistik.

### Charts dan strategi

Tab Charts memilih 2–6 driver dan menyediakan sembilan chart §7: race pace (fuel correction dan rolling 3 laps), tyre degradation, gap evolution/catch estimate, gap to leader, sector deltas (rentang N lap), position, stint strategy, speed traps, serta prediction vs actual. Pit/SC outliers ditandai; warna tim dengan garis solid/dashed membedakan rekan satu tim.

Chart mendukung live append, pan/zoom, crosshair bersama, lap range, pin ke tower, PNG, dan URL yang memulihkan sesi/driver/chart/range. Input URL divalidasi. Share URL tidak berisi kredensial atau raw data. Forecast dibedakan dari observasi dan disertai band/label.

Pemilih pasangan driver menyediakan undercut/overcut simulation menggunakan pit loss, out-lap, pace/deg, dan gap. Hasil menyatakan asumsi dan band. Peta menampilkan estimasi rejoin; projected finish menyertakan distribusi posisi dari minimal 1.000 simulasi per lap.

## Prediksi dan evaluasi

- **P1:** baseline stint degradation + residual model ONNX ketika artefak tersedia; horizon 1–5 lap dan p10/p90. Fallback baseline diberi label.
- **P2:** online Bayesian regression per stint dan changepoint/cliff detection. Fuel, traffic, pit, SC/VSC, dan red-flag laps diperlakukan sesuai aturan cleaning PRD.
- **P3:** model survival/hazard ONNX; fallback distribusi stint historis per compound dengan label. Jika histori tidak cukup, tidak mengarang probabilitas berbasis data.
- **P4:** simulasi undercut/overcut dari P1/P2 dengan asumsi pit/out-lap yang terlihat.
- **P5/P6:** simulasi finishing order dan deterministic rejoin dari pit loss/gaps; forecast tetap membawa ketidakpastian input.
- **P9/P10:** cut-off qualifying berbasis evolution regression dan anomaly rolling z-score. Data tidak cukup menghasilkan unavailable.
- **Insight:** rule templates menjelaskan hasil terukur, tanpa layanan LLM atau klaim SC/overtake terkalibrasi yang tidak terlatih.
- Log menyimpan forecast asli, timestamp/horizon, model version, band, faktor, dan aktual yang dicocokkan kemudian. Seek/replay berulang memakai identitas run dan tidak menggandakan skor.
- Dashboard akurasi menampilkan ukuran sampel, model, split/race, coverage, pace MAE, pit ±2 lap pada horizon 10 lap, serta posisi ±1 di titik 50% distance. Sampel kosong ditampilkan tanpa persentase palsu.
- Pipeline offline mengakuisisi 2023–2025 ke Parquet, membersihkan data, membentuk fitur kausal, grouped cross-validation per race, melatih/mengekspor ONNX, dan backtest melalui replay engine yang sama. Akuisisi dataset, pelatihan P1/P3, artefak terlatih, serta laporan backtest adalah pekerjaan delivery v1, bukan hanya menyediakan CLI/config. CLI dan regression fixture juga diuji. Artefak mencatat training split, feature schema, dan versi agar serving menolak model tidak kompatibel. Jika dataset/komputasi belum memungkinkan penyelesaian, bagian ini tetap terbuka dan implementasi tidak dilaporkan memenuhi seluruh PRD.

## Operasional, keamanan, dan lisensi

REST hanya menyediakan katalog dan tampilan turunan yang diperlukan UI (history/telemetry terpilih), tidak menjadi proxy bebas endpoint OpenF1 atau dump raw feed. Rate limit per IP, batas request/rentang/subscription/payload, timeout, retry/backoff, dan antrean bounded melindungi server serta batas sumber. Deployment menjelaskan trusted proxy agar header IP tidak dapat dipalsukan.

Cache/session persistence dan resume ingest memiliki watermark/checkpoint setelah transaksi berhasil. Redis tidak menjadi satu-satunya penyimpan durable. Migrasi database versi-terkontrol; kegagalan tidak menghapus data. Operasi bersih-bersih destruktif tidak dijalankan otomatis. Kredensial dalam environment server; contoh konfigurasi hanya placeholder.

Prometheus dan tracing/OpenTelemetry mengamati feed age, ingest-to-publish latency, inference latency, queue drops, reconnect, resource usage, dan accuracy counts. Tidak ada klaim feed-to-screen p95 tanpa pengukuran browser. Docker/CI/deployment docs menyediakan health checks dan urutan start/migrasi yang dapat diperiksa.

Footer menyatakan unofficial/not affiliated, proyek fan/educational non-commercial, atribusi OpenF1 dan lisensinya. Tidak memakai F1/FIA/team logos. Monetisasi dan pembelian sponsor tidak dilakukan oleh implementasi ini.

## Arah visual dan aksesibilitas

Dashboard timing gelap dengan angka tabular, kepadatan informasi yang terkendali, kontras kuat, dan warna hanya sebagai sinyal tambahan. Hierarki utama: sesi/status, tower, map, kemudian detail. Layout tidak menggunakan landing page pemasaran. Animasi posisi dan map menghormati reduced motion. Keyboard, label form, focus visible, dialog semantics, tooltip yang dapat diakses, dan responsive overflow diverifikasi terhadap WCAG 2.1 AA.

## Pembagian pekerjaan dan commit

Setiap unit memiliki test/proof terkait dan commit sendiri menggunakan format `[type : pesan]`, tanpa trailer/atribusi Codex. Rencana rinci dibuat setelah desain disetujui.

1. Dokumentasikan baseline PRD dan desain, lalu fondasi workspace/config/CI.
2. Domain serta ingest/replay normalization dengan fixture yang jelas asalnya dan dataset metadata sirkuit terverifikasi.
3. State engine, checkpoint, store, dan aturan replay/seek.
4. Model baseline, kontrak ONNX, simulasi, log, dan evaluasi prediksi.
5. API REST/WebSocket, subscription, reconnect, rate limit, dan metrik.
6. Shell frontend, pemilih sesi/playback, tower, map, race control, serta layout persistence.
7. Panel telemetry, overlay, beginner/expert, dan race history.
8. Sembilan charts, perbandingan strategi, export/share/pin, serta accuracy view.
9. Akuisisi dataset/pipeline offline/pelatihan/model serving/backtest, Docker/deployment docs, dan penerimaan e2e lintas lapisan.

Urutan dapat dipecah lebih kecil berdasarkan dependensi; tidak mencampur perubahan tak terkait. PRD tidak diubah untuk menyesuaikan kekurangan implementasi. Requirement checklist menghubungkan §4–11 dengan kode/test dan mencatat kekurangan material secara terbuka.

## Bukti penerimaan

- **Rust unit/regression:** normalisasi/validasi input, event duplicate/out-of-order, lap/stint/pit transitions, bounded buffer, red flag/SC/rain confidence, band dan faktor, no future leakage, deterministic replay seek, stale inference rejection, serta forecast-vs-actual scoring.
- **Integration:** sumber fixture -> state -> persistence -> WebSocket binary snapshot/delta -> subscription telemetry; koneksi kembali/resync dan kegagalan sumber. Replay dua klien saling terisolasi. Migrasi dan restart/checkpoint diuji dengan database lokal/container yang tersedia.
- **Browser e2e:** pilih sesi -> play -> lap berubah -> tower/prediksi/chart berubah -> klik driver -> telemetry -> overlay dan expert -> pilih 2–6 driver -> undercut -> pin/export/share -> pause/speed/seek -> reload layout. Uji pula empty/error/delayed, keyboard, dan viewport ponsel.
- **Cakupan data/model:** periksa kelengkapan metadata konfigurasi sirkuit terhadap katalog sejak 2023, fastest-driver mini-sector map, artefak ONNX P1/P3 yang benar-benar terlatih, dan laporan backtest per race. Pipeline yang hanya diuji dengan fixture tidak menutup penerimaan model terlatih.
- **Live smoke:** hanya dengan akses OpenF1 yang sah; tanpa akses itu pengujian live dicatat belum terverifikasi. Historical smoke tidak menggantikan uji transport live berautentikasi.
- **Gates:** cargo fmt/clippy/test, TypeScript typecheck, frontend build, browser e2e, serta pemeriksaan diff dan review sebelum klaim selesai/commit. Uji hanya yang relevan dengan perubahan masing-masing.
- **Performa/akurasi:** harness mengukur p95 ingest-to-screen, prediction <500 ms, panel <300 ms, bandwidth <20 KB/s, dan kapasitas/load. Target 10.000 klien, 60 fps, load 4G, kompatibilitas lintas browser, SLA 99,5%, serta akurasi backtest tidak diklaim lulus tanpa run representatif. Bukti dan batas lingkungan dicatat dalam laporan akhir.

## Kondisi lingkungan dan hal yang menunggu

Repo awal hanya `PRD.md` dan belum memiliki Git. Git user sudah dikonfigurasi. Rust tersedia di direktori cargo pengguna (belum berada di PATH shell), Node 22/npm tersedia. Docker CLI tersedia tetapi daemon tidak berjalan pada pemeriksaan awal. Startup Docker atau PostgreSQL lokal diperiksa saat implementasi; kegagalan infrastruktur dilaporkan tanpa memalsukan hasil integrasi.

Persetujuan yang diminta sekarang adalah desain ini. Kredensial sponsor, model hasil pelatihan, dan hasil pengukuran produksi merupakan prasyarat verifikasi bagian terkait, bukan alasan mengganti fitur dengan data palsu atau menganggap seluruh PRD telah terpenuhi.
