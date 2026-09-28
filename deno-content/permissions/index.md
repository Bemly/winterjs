---
title: "Deno.permissions"
slug: Deno/permissions
---

Deno's permission management API.
It is a singleton instance of the Permissions object and is based on the web platform [Permissions API](https://developer.mozilla.org/en-US/docs/Web/API/Permissions_API), though some proposed parts of the API which are useful in a server side runtime context were removed or abandoned in the web platform specification which is why it was chosen to locate it in the Deno namespace instead.
By default, if the stdin/stdout is TTY for the Deno CLI (meaning it can send and receive text), then the CLI will prompt the user to grant permission when an un-granted permission is requested. This behavior can be changed by using the --no-prompt command at startup. When prompting the CLI will request the narrowest permission possible, potentially making it annoying to the user. The permissions APIs allow the code author to request a wider set of permissions at one time in order to provide a better user experience.
Requesting already granted permissions will not prompt the user and will return that the permission was granted.
### Querying
### Revoking
### Requesting

## Syntax

```ts
export const permissions: Permissions;
```
