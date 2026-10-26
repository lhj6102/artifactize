# 백엔드와 로그인 {#backends-and-sign-in}

Agent eval은 백엔드를 통해 모델 서비스에 리뷰를 요청하며, Runtime eval과 Human eval에는 백엔드가 필요하지 않습니다.
Agent eval이 쓰는 백엔드만 준비하세요. OpenAI, Anthropic, Gemini API 키나 Codex가 포함된
ChatGPT 요금제 중 필요한 것을 쓰면 됩니다.

Agent eval에는 백엔드 하나와 사용할 모델의 정확한 ID를 지정하세요. artifactize는
모델 목록을 따로 제공하지 않으며, 지정한 백엔드나 모델을 다른 것으로 바꾸지도 않습니다.
아래 기본 백엔드 말고도 직접 이름을 붙인 [커스텀 provider](../features/agent.md#custom-providers)를
백엔드로 쓸 수 있습니다. 기본 백엔드의 별칭이나, OpenAI·Anthropic·Gemini와 호환되는 엔드포인트를
등록하면 됩니다.

**백엔드**는 artifactize가 Agent 리뷰를 모델 서비스로 보낼 때 쓰는
연결입니다. **API 키**는 그 서비스에서 계정을 인증할 때 쓰는 비공개 인증 정보입니다.
API 키 백엔드를 쓰려면 [OpenAI 계정](https://platform.openai.com/api-keys),
[Anthropic Console](https://console.anthropic.com/settings/keys),
[Google AI Studio](https://aistudio.google.com/apikey)에서 키를 만든 다음 `artifactize login`으로
저장하세요. 키를 묻는 프롬프트에 붙여 넣으면 artifactize가 stdin에서 키를 읽습니다. 데스크톱 앱의
**Settings → Providers**에 붙여 넣어 저장해도 됩니다. 키를 커밋하거나 Artifact 선언에 넣지 마세요.
Codex는 대신 아래의 별도 브라우저 로그인을 씁니다.

| 백엔드 | 설정 | 모델 목록 |
|---|---|---|
| `openai` | `artifactize login openai` | `artifactize models openai` |
| `anthropic` | `artifactize login anthropic` | `artifactize models anthropic` |
| `gemini` | `artifactize login gemini` | `artifactize models gemini` |
| `codex` | `artifactize login codex` | `artifactize models codex` |

`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`에 키가 있으면 저장한 키보다 먼저 씁니다.
예를 들어 `export OPENAI_API_KEY=...`처럼 설정하세요(PowerShell에서는
`$env:OPENAI_API_KEY = "..."`). 저장한 키는 `logout openai|anthropic|gemini`로 지웁니다.

`login codex`는 Codex CLI와 같은 방식으로 ChatGPT 계정에 로그인합니다.
로그인 URL을 출력하고 기본 브라우저로 엽니다(Linux와 WSL에서는 `xdg-open`, macOS에서는 `open`,
Windows에서는 기본 브라우저). 로그인은 같은 머신의 브라우저에서 마치세요.
로그인이 끝나면 브라우저가 `localhost:1455`로 이동합니다. WSL 2에서는 Windows 브라우저도 쓸 수 있습니다. 아무것도 열리지 않으면
출력된 URL을 브라우저에 복사하세요. Codex CLI나 다른 앱의 로그인 기능이 포트 1455를
사용 중이면, artifactize가 주소를 붙여 넣으라고 안내합니다. 다른 머신에서 로그인할 때도 URL을 붙여 넣는 방법을 쓰세요.
`http://localhost:1455/auth/callback?code=`로 시작하는 전체 URL을
터미널에 붙여 넣거나 데스크톱 앱의 로그인 창에 있는 URL 입력란에 붙여 넣으세요. 로그인은 5분 안에 마쳐야 하며, 그 뒤에는
`login codex`를 다시 실행하세요. 페이지 로드에 실패했다면 브라우저 주소 표시줄에서,
페이지가 계속 로딩 중이라면 개발자 도구 → Network에서 이 URL을 찾을 수 있습니다.
`logout codex`는 artifactize의 토큰을 삭제하고
서버에 토큰 폐기를 요청합니다. 폐기가 확인되지 않으면 경고합니다. 대신 기존 Codex 로그인의 토큰을 쓰려면
`ARTIFACTIZE_CODEX_AUTH_FILE=~/.codex/auth.json`을 설정하세요. artifactize는 그 파일을 읽기만 합니다.
[Codex](../features/agent.md#codex)를 보세요.

```sh
artifactize doctor                 # state and its schema, API-key presence, Codex sign-in, review store, desktop app, newer release
artifactize doctor --repo PROJECT  # also validates PROJECT's declarations
```

`doctor`는 어떤 provider도 호출하지 않고 Run도 만들지 않습니다. 모든 백엔드가 선택 사항이므로
키나 로그인이 없으면 경고로 처리합니다. 데스크톱 앱이 없을 때도 설치 링크와 함께 경고만 하고, [새 릴리스](update.md#newer-releases)가 나온 경우도 경고입니다. 쓸 수 없는 상태나 잘못된 설정 같은
심각한 오류가 있을 때만 1로 종료합니다.
