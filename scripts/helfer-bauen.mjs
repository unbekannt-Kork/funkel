// Baut den Helfer (lokales Sprachmodell) und legt ihn so ab, wie Tauri ihn als
// mitgeliefertes Programm erwartet: src-tauri/binaries/funkel-helfer-<ziel>[.exe]
import { execSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";

const ziel = execSync("rustc -vV").toString().match(/host: (\S+)/)[1];
execSync("cargo build --release --manifest-path helfer/Cargo.toml", { stdio: "inherit" });
const endung = process.platform === "win32" ? ".exe" : "";
mkdirSync("src-tauri/binaries", { recursive: true });
copyFileSync(`helfer/target/release/funkel-helfer${endung}`, `src-tauri/binaries/funkel-helfer-${ziel}${endung}`);
console.log(`Helfer für ${ziel} abgelegt`);
