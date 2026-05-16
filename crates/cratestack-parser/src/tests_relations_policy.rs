#![cfg(test)]

use super::parse_schema;

#[test]
fn preserves_recursive_relation_policy_attributes() {
    let schema = parse_schema(
        r#"
auth SessionUser {
  email String
  orgSlug String
}

model Organization {
  id Int @id
  slug String
}

model User {
  id Int @id
  email String
  banned Boolean
}

model Project {
  id Int @id
  organizationId Int
  organization Organization @relation(fields:[organizationId],references:[id])
  memberships Membership[] @relation(fields:[id],references:[projectId])
}

model Membership {
  id Int @id
  projectId Int
  userId Int
  active Boolean
  blocked Boolean
  project Project @relation(fields:[projectId],references:[id])
  user User @relation(fields:[userId],references:[id])
}

model Task {
  id Int @id
  projectId Int
  project Project @relation(fields:[projectId],references:[id])

  @@deny("read", project.memberships.some.user.banned)
  @@allow("read", project.organization.slug == auth().orgSlug && project.memberships.some.user.email == auth().email)
  @@allow("delete", project.memberships.every.active)
  @@allow("create", project.memberships.none.blocked)
}
"#,
    )
    .expect("recursive policy schema should parse");

    let task = schema
        .models
        .iter()
        .find(|model| model.name == "Task")
        .expect("task model should parse");
    assert_eq!(task.attributes.len(), 4);
    assert!(
        task.attributes[0]
            .raw
            .contains("project.memberships.some.user.banned")
    );
    assert!(
        task.attributes[1]
            .raw
            .contains("project.organization.slug == auth().orgSlug")
    );
    assert!(
        task.attributes[2]
            .raw
            .contains("project.memberships.every.active")
    );
    assert!(
        task.attributes[3]
            .raw
            .contains("project.memberships.none.blocked")
    );
}
