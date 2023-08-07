
;; title: px
;; version:
;; summary:
;; description: Allows users to pay to update data in a matrix. 
;;  Each matrix value must be a hexadecimal value from 0x000000 to 0xffffff, representing a color to be displayed on a grid in a web page. 
;;  Each matrix key corresponds to the location of the grid, which is 100x100 cells.

;; traits
;;

;; token definitions
;; 

;; constants
;;
(define-constant MAX_LOC u100)
(define-constant MAX_VAL 0xffffff)
(define-constant MIN_VAL 0x000000)
(define-constant ALL_LOCS (list u0 u1 u2 u3 u4 u5 u6 u7 u8 u9 u10 u11 u12 u13 u14 u15 u16 u17 u18 u19 u20 u21 u22 u23 u24 u25 u26 u27 u28 u29 u30 u31 u32 u33 u34 u35 u36 u37 u38 u39 u40 u41 u42 u43 u44 u45 u46 u47 u48 u49 u50 u51 u52 u53 u54 u55 u56 u57 u58 u59 u60 u61 u62 u63 u64 u65 u66 u67 u68 u69 u70 u71 u72 u73 u74 u75 u76 u77 u78 u79 u80 u81 u82 u83 u84 u85 u86 u87 u88 u89 u90 u91 u92 u93 u94 u95 u96 u97 u98 u99))
;; data vars
;;

;; data maps
;;
(define-map pixels uint (buff 3))

;; public functions
;;
(define-public (set-value-at (loc uint) (value (buff 3))) 
    (begin 
        (if (>= loc MAX_LOC)
            (err "Location out of bounds.")
            (if (> value MAX_VAL)
                (err "Value must be less than 0xffffff.")
                (if (< value MIN_VAL)
                    (err "Value must be greater than 0x000000.")
                    (ok (map-set pixels loc value))
                )
            )
        )
    )
)
;; read only functions
;;

(define-read-only (get-value-at (loc uint))
    (if (>= loc MAX_LOC)
        (err "Out of bounds.")
        (ok (default-to 0xffffff (map-get? pixels loc)))
    )
)

(define-read-only (get-all) 
    (map get-value-at ALL_LOCS)
)

(define-read-only (genesis-time (height uint))
    (get-block-info? time height)
)
;; private functions
;;
