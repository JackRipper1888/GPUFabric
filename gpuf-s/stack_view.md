## performance analysis
### Runtime
```bash
cargo run --debug 
```
use dhat to profile
```bash
dhat-heap.json
```
use dh_view to view
```bash
https://nnethercote.github.io/dh_view/dh_view.html
```
### Compilation (in linux)
```bash
export RUSTFLAGS="-C opt-level=3 -C llvm-args=-fstack-usage -C save-temps"
cargo build
```