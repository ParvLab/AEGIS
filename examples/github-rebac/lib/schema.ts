export const SCHEMA = `
types:
  user: {}
  org:
    relations:
      member: {}
      admin:
        inherit_from:
          - member
    permissions:
      view:
        union_of:
          - member
          - admin
      manage:
        union_of:
          - admin
  team:
    relations:
      member: {}
      maintainer:
        inherit_from:
          - member
      admin:
        inherit_from:
          - maintainer
    permissions:
      pull:
        union_of:
          - member
          - maintainer
          - admin
      push:
        union_of:
          - maintainer
          - admin
      admin:
        union_of:
          - admin
  repo:
    relations:
      viewer: {}
      maintainer:
        inherit_from:
          - viewer
      admin:
        inherit_from:
          - maintainer
      banned: {}
    permissions:
      pull:
        union_of:
          - viewer
          - maintainer
          - admin
      push:
        union_of:
          - maintainer
          - admin
      admin:
        union_of:
          - admin
    deny:
      - relations:
          - banned
        description: "Banned users are denied all access to this repo"
`;
