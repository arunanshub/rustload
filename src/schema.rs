table! {
    badexes (id) {
        id -> Integer,
        update_time -> Integer,
        uri -> Text,
    }
}

table! {
    exemaps (id) {
        id -> Integer,
        seq -> Integer,
        map_seq -> Integer,
        time -> Integer,
    }
}

table! {
    exes (id) {
        id -> Integer,
        seq -> Integer,
        update_time -> Integer,
        time -> Integer,
        uri -> Text,
    }
}

table! {
    maps (id) {
        id -> Integer,
        seq -> Integer,
        update_time -> Integer,
        offset -> Integer,
        uri -> Text,
    }
}

table! {
    markovs (id) {
        id -> Integer,
        a_seq -> Integer,
        b_seq -> Integer,
        time -> Integer,
        time_to_leave -> Binary,
        weight -> Binary,
    }
}

allow_tables_to_appear_in_same_query!(badexes, exemaps, exes, maps, markovs,);
