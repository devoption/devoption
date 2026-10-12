use crate::*;

#[cfg(unix)]
mod registration;
#[cfg(unix)]
mod storage;

fn fixture() -> Catalog {
    let mut catalog = Catalog::default();
    let first = catalog.create_project("Application").unwrap();
    let second = catalog.create_project("Documentation").unwrap();
    let frontend = catalog.create_repository("Frontend").unwrap();
    let backend = catalog.create_repository("Backend").unwrap();
    catalog.associate(first, frontend).unwrap();
    catalog.associate(first, backend).unwrap();
    catalog.associate(second, backend).unwrap();
    catalog
}

#[test]
fn identities_and_associations_are_independent_of_names_and_cardinality() {
    let mut catalog = fixture();
    let first = catalog.projects()[0].id();
    let second = catalog.projects()[1].id();
    let frontend = catalog.repositories()[0].id();
    let backend = catalog.repositories()[1].id();
    assert_ne!(first, second);
    assert_ne!(frontend, backend);
    assert_eq!(catalog.projects()[0].repositories(), &[frontend, backend]);
    assert_eq!(catalog.projects()[1].repositories(), &[backend]);
    assert!(matches!(
        catalog.associate(first, frontend),
        Err(Error::AlreadyAssociated)
    ));
    let other = fixture();
    assert!(matches!(
        catalog.associate(other.projects()[0].id(), backend),
        Err(Error::UnknownProject)
    ));
    assert!(matches!(
        catalog.associate(first, other.repositories()[0].id()),
        Err(Error::UnknownRepository)
    ));
    let same_name = catalog.create_project("Application").unwrap();
    assert_ne!(same_name, first);
    assert_eq!(catalog.projects()[0].id(), first);
    assert_eq!(catalog.projects()[0].name(), "Application");
}

#[test]
fn invalid_or_oversized_names_do_not_change_the_catalog() {
    let mut catalog = fixture();
    let original = catalog.clone();
    for name in [
        "",
        " ",
        " padded",
        "padded ",
        "line\nbreak",
        "nul\0",
        &"é".repeat(65),
    ] {
        assert!(matches!(
            catalog.create_project(name),
            Err(Error::InvalidName)
        ));
        assert!(matches!(
            catalog.create_repository(name),
            Err(Error::InvalidName)
        ));
        assert_eq!(catalog, original);
    }
    // Names are stored as data, not interpreted or silently HTML-encoded.
    catalog.create_project("<研究 & tools>").unwrap();
    catalog.create_repository(&"é".repeat(64)).unwrap();
}

#[test]
fn catalog_capacity_is_bounded_without_limiting_projects_to_one_repository() {
    let mut catalog = Catalog::default();
    for _ in 0..MAX_PROJECTS {
        catalog.create_project("Project").unwrap();
    }
    for _ in 0..MAX_REPOSITORIES {
        catalog.create_repository("Repository").unwrap();
    }
    assert!(matches!(
        catalog.create_project("Overflow"),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        catalog.create_repository("Overflow"),
        Err(Error::Capacity)
    ));
    let project = catalog.projects()[0].id();
    let repositories: Vec<_> = catalog.repositories().iter().map(Repository::id).collect();
    for repository in repositories {
        catalog.associate(project, repository).unwrap();
    }
    assert_eq!(catalog.projects()[0].repositories().len(), MAX_REPOSITORIES);
}
