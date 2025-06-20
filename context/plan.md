# claco

cla(ude)co(de) is claude code cli inspector.

features:
1. history command: list all user input messages under this project, input{timestamp: message}
2. session command: with session id then print info about session{first-user-message:text, timestamp}
3. projects command: list all projects{project-folder, sessions:array<session-id>}
4. live command: list all living session{project}

## learn from claude code

### structure under $HOME/.claude folder
- ide: live claude process, filename pattern `$pid.lock`
- projects: store each projects message list
- statsig: unknown
- todos: unknown

### project directory name

```
// (pseudocode reconstructed from the bundled file)
const projectRoot = path.join(os.homedir(), '.claude', 'projects');
const key = cwd                      // e.g.  /Users/kaichen/workspace/claco
  .replace(/^\/+/, '')               // drop leading “/”
  .replace(/[\\/:]+/g, '-')          // → Users-kaichen-workspace-claco
  .replace(/^-+|-+$/g, '');          // trim extra dashes
const projectDir = path.join(projectRoot, key);
fs.mkdirSync(projectDir, {recursive:true});
```

### project jsonl log

```
const logfile = path.join(projectDir, `${sessionId}.jsonl`);
const stream  = fs.createWriteStream(logfile, {flags:'a'});
const record = {
  parentUuid,            // threading / follow-up
  isSidechain,           // true when spawned by an Agent tool
  userType,              // 'external' | 'assistant' | 'system'
  cwd,                   // absolute working directory
  sessionId,             // same UUID as filename
  version: pkg.version,  // CLI version
  type,                  // 'user' | 'assistant' | 'system'
  message,               // full Anthropic message object
  uuid: crypto.randomUUID(),
  timestamp: new Date().toISOString()
};
stream.write(JSON.stringify(record) + '\n');
```

## example data

```ide/34946.lock
{"pid":92912,"workspaceFolders":["/Users/kaichen/workspace/claco"],"ideName":"Visual Studio Code - Insiders","transport":"ws","authToken":"7a9c7d4e-4444-3333-2222-189b80a4e3dd"}
```

```projects/-Users-kaichen-workspace-claco/48fb8f8e-48e9-4eb8-b035-4b72deb386cf.jsonl
{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"/Users/kaichen/workspace/claco","sessionId":"48fb8f8e-48e9-4eb8-b035-4b72deb386cf","version":"1.0.29","type":"user","message":{"role":"user","content":"init basic cli project structure"},"uuid":"65616ffb-bddc-4a08-b5e2-dd128856d822","timestamp":"2025-06-19T16:48:09.734Z"}
{"parentUuid":"7371862e-4b5a-440e-bd0f-c39256786433","isSidechain":false,"userType":"external","cwd":"/Users/kaichen/workspace/claco","sessionId":"48fb8f8e-48e9-4eb8-b035-4b72deb386cf","version":"1.0.29","type":"user","message":{"role":"user","content":"remove sub commands, keep one main command for everything."},"uuid":"7ba0616a-f38b-4bfd-9aa6-84d087548939","timestamp":"2025-06-19T16:57:29.479Z"}
```
