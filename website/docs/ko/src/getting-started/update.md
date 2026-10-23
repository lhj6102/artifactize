# 업데이트와 제거 {#update-and-uninstall}

## 업데이트 {#update}

| 설치 방법 | 업데이트 |
|---|---|
| `install.sh` 또는 `install.ps1` | 같은 명령을 다시 실행하세요. 데스크톱 앱도 함께 업데이트됩니다 |
| Homebrew | `brew upgrade artifactize`. 데스크톱 앱은 `brew upgrade --cask artifactize` |
| Microsoft Store | 자동. 또는 Store 앱의 **라이브러리 → 업데이트 받기** |
| 디스크 이미지, `.deb` 또는 AppImage | 최신 릴리스의 설치 파일을 같은 방법으로 설치하세요 |
| `cargo binstall` | `cargo binstall artifactize artifactize-tools` |
| `cargo install` | `cargo install artifactize artifactize-tools --locked`. 체크아웃에서는 `git pull` 후 `cargo install --path`를 반복하세요 |

설치 스크립트나 Cargo를 쓰면 릴리스가 게시되는 즉시 그 버전을 설치할 수 있습니다. Homebrew와
Microsoft Store는 안정 릴리스만 제공하며, Microsoft가 검토하는 동안 Store 업데이트가
늦어질 수 있습니다. 업데이트해도 상태는 유지됩니다.

플러그인 업데이트 방법은 [플러그인 페이지](../agent-plugin/plugin.md#updates)에 있습니다.

### 0.11에서 업그레이드 {#upgrading-from-011}

저장된 리뷰 데이터의 형식(스키마)은 처음 사용할 때 스키마 6~8에서 현재 스키마 9로 자동으로
업그레이드되며, 기존 Run, 캐시 기록, Human 승인은 그대로 남습니다. 이전 버전의 artifactize는
업그레이드된 상태를 열 수 없으니, 버전을 바꾸기 전에 백업해 두세요.

이제 `artifactize verify`는 터미널을 닫아도 계속 실행되는 Run을 제출하고, Run ID를 출력한 뒤 접수되면 종료합니다.
검증 종료 코드가 필요한 스크립트와 CI에는 `--wait`를 추가하세요. 터미널을 닫거나
지켜보는 명령을 중단해도 Run은 계속 실행됩니다. Run을 취소하려면
`artifactize run cancel RUN_ID`를 실행하세요. 포그라운드 Human 제한 시간과
`verify --timeout-ms`는 제거되었습니다. `--request-key`를 쓰면 재시도해도 같은 Run이 반환됩니다.

게이트 우회(`review_policy.dependency_gates`를 포함한 `--ignore-gates`와 `review_policy`)는
제거되었습니다. 빈 `review_policy` 테이블도 설정 검사에 실패하니 지우세요. 요청마다 고르는
`--speculate`와 `--defer-human`이 이를 대신하며, 둘 다 기본으로 꺼져 있고 어느 쪽도 최종 승인을
면제하지 않습니다. 승인을 미루면 Run은 `SIGNOFFS_PENDING`으로 끝나고 `--wait`는 5로 종료합니다. [Run과 status](../features/run.md)를 보세요.
인자 없이 실행한 `artifactize monitor`는 ALL에서 시작합니다. `monitor .`, 위치 인자 경로, 또는 `--repo`는
처음에 그 저장소를 선택합니다.

### 0.9로 업그레이드 {#upgrading-to-09}

선언을 폴더에는 TOML `index.artf`로, 파일에는 `<file>.artf`로 다시 작성하세요. 키는
snake_case이고 eval은 `[evals.<id>]` 테이블입니다. `.artfignore`가 예전 무시 파일을
대신합니다. 변환기는 없습니다. 에이전트에게 파일을 다시 작성해 커밋하게 하고,
예전 표시를 제거한 뒤 `artifactize config check`를 실행하세요. Family는 제거되었습니다. 템플릿이
필요하다면 직접 만든 생성기를 쓰고 그 `.artf` 출력을 커밋하세요. [선언 (.artf)](../features/declarations.md)을 보세요.

0.9 이전의 상태(스키마 1~5)는 거부합니다. `ARTIFACTIZE_STATE_HOME`이나 `--state-dir`을 새
디렉터리로 설정하거나, 이전 디렉터리를 다른 곳으로 옮기세요. 그 뒤의 상태는 자동으로
업그레이드됩니다([상태](../features/others/state.md#state)).
재사용 키 v2는 0.9 이전의 어떤 결과와도 일치하지 않습니다.
팀 리뷰 저장소에 있거나 스크립트 fingerprint를 쓰는 경우도 마찬가지입니다. 업그레이드 후 처음으로 전체 `verify`를
실행하면 Human 승인을 포함해 모든 eval을 한 번씩 리뷰합니다. [0.9로 업그레이드](../features/fingerprint.md#upgrading-to-09)를 보세요.

이제 Monitor는 eval의 Detail을 모달이 아니라 전용 패널에서 엽니다. 독립 실행형
Review도 같은 Human Detail을 씁니다. 요청은 직접 맡아야 하며, 도구를 실행하거나
결과를 제출해도 요청을 맡은 것으로 처리되지 않습니다. 판정 양식, 제출 키, 편집기 단축키가 바뀌었습니다.
현재 조작 방법은 [Human 리뷰와 Monitor](../features/human.md)를 보세요.

0.5.0은 `chatgpt`와 `claude` Agent 백엔드를 `login chatgpt`,
`logout chatgpt`, 내부 `mcp` 명령과 함께 제거하고, ChatGPT 요금제
사용자를 위한 `codex`를 추가합니다. `config check`는 제거된 백엔드를 아직 쓰는
eval이나 프로필 변형을 찾아 대신 쓸 백엔드를 안내합니다. artifactize는 이전 릴리스가 저장한 ChatGPT
토큰을 더 이상 쓰지 않습니다. ChatGPT 설정에서 artifactize 연결을 끊은 다음
상태 디렉터리에서 `auth/chatgpt.json`, `auth/chatgpt-registration.json`, `auth/chatgpt.lock`을
삭제하세요.

## 상태 {#state}

Run, 저장된 Agent 대화, 로그인을 포함한 모든 상태는 한
디렉터리에 있습니다. 기본값은 `~/.local/state/artifactize`(Windows에서는 `%LOCALAPPDATA%\artifactize`)입니다.
[상태](../features/others/state.md#state)에 그 내용,
상태 위치를 옮기는 환경 변수, Store 버전의 전용 사본이 정리되어 있습니다.

`--state-dir PATH`는 그 명령 하나에서만 상태 위치 전체를 바꿉니다. `login`, `verify`, `request`, `run`,
`monitor`에 같은 값을 쓰세요. 상태는 리뷰 대상 저장소 밖에 있어야 합니다.
`auth/` 아래의 토큰도 git 워크트리나
artifactize 프로젝트 안에는 둘 수 없습니다. `$HOME`에 있는 dotfiles 저장소나 체크아웃 안의 gitignore된 폴더가 그런 예입니다.
`ARTIFACTIZE_CODEX_AUTH_FILE`을 쓰면 토큰을 따로 저장할 곳이 필요 없습니다
([상태](../features/others/state.md#state)).

## 정리와 제거 {#cleanup-and-uninstall}

```sh
artifactize prune --dry-run           # list removable scratch output of finished Runs
artifactize prune --older-than 7d     # remove it; saved results stay readable
artifactize logout codex              # if you signed in with Codex
artifactize remote logout             # if you signed in to a review store
```

그다음 설치한 방법대로 명령을 제거하세요.

```sh
rm ~/.local/bin/artifactize ~/.local/bin/artifactize-tools   # install.sh (or your ARTIFACTIZE_INSTALL_DIR)
brew uninstall artifactize                                   # Homebrew
cargo uninstall artifactize artifactize-tools                # cargo install or cargo binstall
```

데스크톱 앱도 제거합니다.

```sh
rm -rf ~/Applications/Artifactize.app                        # install.sh on macOS
rm -rf ~/.local/share/artifactize/desktop ~/.local/bin/artifactize-desktop \
  ~/.local/share/applications/artifactize.desktop             # install.sh on Linux
brew uninstall --cask artifactize                            # Homebrew cask, with its commands
sudo apt remove artifactize                                  # .deb
```

디스크 이미지로 설치했다면 응용 프로그램 폴더의 **Artifactize**를 휴지통으로 끌어다 놓으세요.
첫 실행 때 명령 설정을 승인했다면 그 터미널 경로 항목도 제거하세요.

```sh
sudo rm -f /etc/paths.d/artifactize
```

AppImage를 단독으로 실행했다면 AppImage를 삭제하세요. 앱의 고정 복사본을 가리키는
명령 링크만 제거한 다음, 그 복사본을 제거하세요.

```sh
bin="${XDG_DATA_HOME:-$HOME/.local/share}/artifactize/bin"
for command in artifactize artifactize-tools; do
  link="$HOME/.local/bin/$command"
  [ "$(readlink "$link")" != "$bin/$command" ] || rm "$link"
done
rm -rf "$bin"
```

명령의 경로 항목을 제거한 뒤에는 새 터미널을 여세요.

Windows에서는:

```powershell
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\artifactize"   # install.ps1
Remove-Item "$([Environment]::GetFolderPath('Programs'))\Artifactize.lnk"   # its Start menu shortcut
```

Store 버전은 **설정 → 앱 → 설치된 앱**에서 제거하거나, 시작 메뉴에서
Artifactize를 마우스 오른쪽 버튼으로 클릭해 제거하세요. `install.ps1` 설치를 제거한 뒤에는
그 디렉터리를 사용자 `Path`에서도 제거하세요(시작 메뉴에서 "계정의 환경 변수 편집"을
검색하세요).

**Store 앱**을 제거하면 Run, 저장된 결과, 로그인, 캐시를 포함한
패키지 전용 상태가 제거됩니다. 다른 설치 방법은 상태 디렉터리를 남겨 두므로,
나중에 패키지되지 않은 버전을 설치하면 그 Run과 결과를 다시 볼 수 있습니다. `prune`은 활성 Run,
데이터베이스 행, 저장소를 건드리지 않습니다. 패키지되지 않은 버전이 남긴 상태를 지우려면
`artifactize doctor`가 첫 줄에 출력하는 상태 디렉터리를 삭제하세요.

제거해도 프로젝트 파일, 내보낸 파일, `--state-dir`이나 환경 변수로 경로를 지정해 AppData 밖에 명시적으로 둔 상태는 지워지지 않습니다. eval의 명령이 만든 파일은 AppData 안에 있더라도 남으니,
더 이상 필요하지 않으면 따로 지우세요. 가상화되지 않은 기존
AppData 파일도 Store 패키지의 정리 대상이 아닙니다.
[Windows 동작과 오버레이 주의 사항](../features/others/state.md#state)을 보세요.
