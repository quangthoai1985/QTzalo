# Developer & Research Scripts

| Script | Mục đích |
|---|---|
| `check-windows.sh` | Kiểm tra (hoặc build với `--build`) bản Windows ngay trên Linux bằng MinGW-w64 (`x86_64-pc-windows-gnu`). Tự cài target Rust và `mingw-w64` nếu thiếu. Bản phát hành chính thức vẫn build bằng MSVC trên CI. |
| `decode_voip_log.py` | Giải mã/tóm tắt file `voip.log` (chỉ có khi chạy với `--dev`). |
| `grab.py` | Trích xuất và lọc các gói tin Zalo quan trọng từ file ghi lưu lượng `zalo.flow` (mitmproxy). |
| `test_pc_status.py` | Kiểm tra nhanh trạng thái phản hồi của endpoint xác nhận QR `pc-status` từ `zalo.flow`. |
| `shot-demo.ps1` | Chạy `zalo-rust.exe --gui --demo` (không mạng, không ghi dữ liệu) và chụp cửa sổ bằng PrintWindow: `.\scripts\shot-demo.ps1 -Exe (Resolve-Path .\target\debug\zalo-rust.exe) -Out shot.png -Theme dark`. Đặt sẵn `QTZALO_DEMO_THREAD=0`, `QTZALO_DEMO_REACT_BAR=1`, `QTZALO_DEMO_SIZE=1280x800`. |
| `re/asar_map.py` | Phần B (Sync V1): đọc header `app.asar` của Zalo PC (không giải nén, không chạy gì), liệt kê module native và file chứa các chuỗi cần tìm: `python -I scripts/re/asar_map.py <app.asar> <out.txt> chuỗi1 chuỗi2 …`. |
| `re/asar_snip.py` | Trích các đoạn ngắn quanh một chuỗi trong một file của `app.asar`: `python -I scripts/re/asar_snip.py <app.asar> <out.txt> <bán kính> <file>=<chuỗi>[,…]`. Kết quả để ngoài repo (`%USERPROFILE%\zalo-re\`). |

File capture (`*.flow`) và log chứa dữ liệu phiên thật: không commit lên repo.
