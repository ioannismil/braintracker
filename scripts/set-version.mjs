// Stamps one version into every manifest. Used by the release workflow:
//   node scripts/set-version.mjs 0.1.42
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("usage: node scripts/set-version.mjs <major.minor.patch>");
  process.exit(1);
}

const editJson = (path) => {
  const json = JSON.parse(readFileSync(path, "utf8"));
  json.version = version;
  writeFileSync(path, JSON.stringify(json, null, 2) + "\n");
};

editJson("package.json");
editJson("src-tauri/tauri.conf.json");

const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8").replace(
  /(\[package\][^[]*?\nversion\s*=\s*)"[^"]*"/,
  `$1"${version}"`,
);
writeFileSync(cargoPath, cargo);

console.log(`version set to ${version}`);
