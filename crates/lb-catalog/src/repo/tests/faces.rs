use crate::repo::test_util::*;
use crate::{CatalogError, NewFace, PhotoQuery, QuerySource};
use lb_core::{FaceSource, NormRect, PeopleKey};

fn face(photo: lb_core::PhotoId, cluster: Option<i64>, score: f32) -> NewFace {
    NewFace {
        photo_id: photo,
        rect: NormRect {
            x: 0.1,
            y: 0.2,
            w: 0.3,
            h: 0.4,
        },
        score,
        embedding: Some(vec![0.6, 0.8]),
        cluster_id: cluster,
        person_id: None,
        source: FaceSource::Detected,
    }
}

#[test]
fn face_scan_stores_embeddings_and_replaces_detected_faces_only() {
    let (_d, c) = open();
    let (_r, folder) = root_and_folder(&c);
    let ids = insert_photos(&c, &folder, 2);
    assert_eq!(c.photos_without_face_scan().unwrap(), ids);

    c.record_face_scan(ids[0], vec![face(ids[0], Some(1), 0.9)], now())
        .unwrap();
    assert_eq!(c.photos_without_face_scan().unwrap(), vec![ids[1]]);
    let manual = c
        .add_manual_face(
            ids[0],
            NormRect {
                x: 0.5,
                y: 0.5,
                w: 0.1,
                h: 0.1,
            },
            None,
        )
        .unwrap();
    let embeddings = c.face_embeddings().unwrap();
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].embedding, vec![0.6, 0.8]);

    c.record_face_scan(ids[0], vec![], now()).unwrap();
    let left = c.faces_of_photo(ids[0]).unwrap();
    assert_eq!(left.len(), 1, "only the manual face survives a rescan");
    assert_eq!(left[0].id, manual);
    assert_eq!(left[0].source, FaceSource::Manual);
}

#[test]
fn naming_merging_and_removing_keep_search_and_groups_consistent() {
    let (_d, c) = open();
    let (_r, folder) = root_and_folder(&c);
    let ids = insert_photos(&c, &folder, 4);
    let f0 = c
        .record_face_scan(ids[0], vec![face(ids[0], Some(1), 0.9)], now())
        .unwrap();
    c.record_face_scan(ids[1], vec![face(ids[1], Some(1), 0.8)], now())
        .unwrap();
    c.record_face_scan(ids[2], vec![face(ids[2], Some(2), 0.95)], now())
        .unwrap();
    c.record_face_scan(ids[3], vec![], now()).unwrap();

    let people = c.list_people(false).unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        people[0].key,
        PeopleKey::Cluster(1),
        "largest unnamed first"
    );
    assert_eq!(people[0].face_count, 2);

    let ana = c.name_cluster(1, "  ana  ").unwrap();
    let again = c.name_cluster(2, "ANA").unwrap();
    assert_eq!(ana.id, again.id, "names are case-insensitive");
    assert_eq!(c.list_people(false).unwrap().len(), 1);
    assert_eq!(
        c.photos_of(PeopleKey::Person(ana.id)).unwrap(),
        vec![ids[0], ids[1], ids[2]]
    );

    c.remove_face_from_person(f0[0]).unwrap();
    let moved = c.get_face(f0[0]).unwrap().unwrap();
    assert_eq!(moved.person_id, None);
    assert!(moved.cluster_id.is_some_and(|cl| cl > 2));
    assert_eq!(
        c.photos_of(PeopleKey::Person(ana.id)).unwrap(),
        vec![ids[1], ids[2]]
    );

    let bob = c
        .name_cluster(moved.cluster_id.unwrap_or_default(), "Bob")
        .unwrap();
    c.merge_people(bob.id, ana.id).unwrap();
    assert!(c.get_person(bob.id).unwrap().is_none());
    assert_eq!(
        c.photos_of(PeopleKey::Person(ana.id)).unwrap(),
        vec![ids[0], ids[1], ids[2]]
    );
    let clusters: std::collections::BTreeSet<_> = c
        .faces_of(PeopleKey::Person(ana.id))
        .unwrap()
        .into_iter()
        .map(|f| f.cluster_id)
        .collect();
    assert_eq!(clusters.len(), 1, "merged people share one cluster");

    let q = PhotoQuery::new(QuerySource::Person(ana.id));
    assert_eq!(c.query_photos(&q).unwrap().len(), 3);

    c.set_person_hidden(ana.id, true).unwrap();
    assert!(c.list_people(false).unwrap().is_empty());
    assert_eq!(c.list_people(true).unwrap().len(), 1);
    c.delete_person(ana.id).unwrap();
    assert_eq!(
        c.list_people(false).unwrap().len(),
        1,
        "faces fall back to an unnamed cluster"
    );
}

#[test]
fn renaming_to_an_existing_name_merges_and_bad_names_are_rejected() {
    let (_d, c) = open();
    let (_r, folder) = root_and_folder(&c);
    let ids = insert_photos(&c, &folder, 2);
    c.record_face_scan(ids[0], vec![face(ids[0], Some(1), 0.9)], now())
        .unwrap();
    c.record_face_scan(ids[1], vec![face(ids[1], Some(2), 0.9)], now())
        .unwrap();
    let a = c.name_cluster(1, "Ana").unwrap();
    let b = c.name_cluster(2, "Bea").unwrap();
    let survivor = c.rename_person(b.id, "ana").unwrap();
    assert_eq!(survivor.id, a.id);
    assert_eq!(c.list_persons().unwrap().len(), 1);
    assert!(matches!(
        c.name_cluster(1, "   "),
        Err(CatalogError::InvalidPersonName)
    ));
    assert!(matches!(
        c.name_cluster(99, "Zed"),
        Err(CatalogError::NotFound("cluster"))
    ));
    assert!(matches!(
        c.assign_face_to_person(lb_core::FaceId::new(999), a.id),
        Err(CatalogError::NotFound("face"))
    ));
}

#[test]
fn picasa_faces_are_removed_without_touching_others() {
    let (_d, c) = open();
    let (_r, folder) = root_and_folder(&c);
    let ids = insert_photos(&c, &folder, 1);
    let rect = NormRect {
        x: 0.2,
        y: 0.2,
        w: 0.2,
        h: 0.2,
    };
    let ana = c.get_or_create_person("Ana").unwrap();
    c.add_picasa_face(ids[0], rect, Some(ana.id)).unwrap();
    c.add_picasa_face(ids[0], rect, None).unwrap();
    c.add_manual_face(ids[0], rect, None).unwrap();
    let faces = c.faces_of_photo(ids[0]).unwrap();
    assert_eq!(
        faces
            .iter()
            .filter(|f| f.source == FaceSource::Picasa)
            .count(),
        2
    );
    assert_eq!(c.remove_picasa_faces(ids[0]).unwrap(), 2);
    let left = c.faces_of_photo(ids[0]).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].source, FaceSource::Manual);
}
