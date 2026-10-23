# Claude Code와 Codex용 플러그인 {#plugin-for-claude-code-and-codex}

**플러그인**은 코딩 어시스턴트에 한 번에 설치할 수 있는 패키지입니다. `artifactize`
플러그인은 Claude Code와 Codex에 두 가지를 함께 제공합니다.

- [Hey Arti](hey-arti.md) 스킬: 어시스턴트가 artifactize로 프로젝트를 구성하고
  검증하게 해 주는 안내
- [artifactize MCP 서버](mcp.md): 어시스턴트가 프로젝트를 살펴보고
  검증 Run을 시작한 뒤 진행 상황을 확인하는 데 쓰는 도구

플러그인은 이 저장소에서 설치합니다. Claude Code와 Codex가 이 저장소를 플러그인
마켓플레이스로 읽으며, 같은 플러그인이 양쪽에서 동작합니다. 플러그인은 프로그램을 설치하지 않고,
훅을 실행하지 않으며, 자격 증명을 담지 않습니다.

## 시작하기 전에 {#before-you-start}

플러그인은 `artifactize` 프로그램을 실행하므로 먼저 [artifactize를 설치](../getting-started/install.md)하세요.
같은 페이지에서 [데스크톱 앱](../desktop/index.md)을 받는 방법도 확인할 수 있습니다. 프로그램이
설치되어 있지 않으면 Hey Arti가 이를 알리고, 설치 페이지를 바탕으로 사용 중인 시스템의 기본 설치 방법을 안내합니다.

## 어시스턴트에 플러그인 추가하기 {#add-the-plugin-to-your-assistant}



Claude Code 세션에서 마켓플레이스를 추가한 다음, 그곳에서 플러그인을 추가하세요.

```text
/plugin marketplace add lhj6102/artifactize
/plugin install artifactize@artifactize
```

셸에서는 `claude plugin marketplace add lhj6102/artifactize`와
`claude plugin install artifactize@artifactize`가 같은 일을 합니다.

Codex에서는 셸에서 다음을 실행하세요.

```sh
codex plugin marketplace add lhj6102/artifactize
codex plugin add artifactize@artifactize
```

또는 Codex CLI에서 `/plugins`를 실행하고 `artifactize` 마켓플레이스를 고르세요.

**Claude Code:** `/reload-plugins`를 실행하거나 새 세션을 시작한 다음, `/mcp`에서
`artifactize` 서버를 확인하세요. 평소 말투로 요청하거나, 스킬을
`/artifactize:hey-arti`로 호출하세요. 서버는 `artifactize --repo <project> mcp`로 실행되며,
`<project>`는 세션이 시작된 디렉터리입니다.

**Codex:** 프로젝트 디렉터리에서 새 Codex 세션을 시작하세요. 서버는
플러그인 폴더에서 `artifactize mcp`로 실행되며, 도구가 프로젝트를 지정하지 않았다고 답하면 Hey Arti가
그 뒤의 호출마다 세션의 프로젝트 폴더를 넘깁니다([서버가 프로젝트를 찾는 방법](mcp.md#which-project)). 평소 말투로 요청하거나,
스킬을 `$artifactize:hey-arti`로 언급하세요.

## 업데이트 {#updates}

플러그인은 릴리스된 버전이 올라가는 이 저장소의 `main` 브랜치를 기준으로 업데이트됩니다.
Claude Code에서는 `/plugin marketplace update artifactize`로 업데이트하세요. `/plugin`에서 마켓플레이스의 자동 업데이트를 켜도 됩니다. Codex에서는 `codex plugin marketplace upgrade artifactize`로 업데이트하세요.
`artifactize` 프로그램 자체를 업데이트하는 방법은 [설치 페이지](../getting-started/install.md#update)에 있습니다.

## 플러그인 없이 {#without-the-plugin}

다른 호스트나 직접 관리하는 설정에서는 서버를 직접 등록하고
([MCP 설정](mcp.md#register-the-mcp-server)), [Hey Arti](hey-arti.md)에 설명된 대로 어시스턴트에
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md)를
제공하세요. 호스트마다 플러그인을 설치하거나 같은 서버를 수동으로 등록하는 방법 중 하나만 쓰세요.
